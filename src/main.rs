// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

#![deny(warnings, missing_docs, clippy::all)]
#![forbid(unsafe_code)]

//! Gnome search provider for Jetbrains products

use std::ffi::OsStr;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use elementtree::Element;
use glib::once_cell::sync::OnceCell;
use regex::Regex;
use tracing::{event, instrument, Level, Span};
use tracing_futures::Instrument;

use gnome_search_provider_common::app::*;
use gnome_search_provider_common::futures_channel::mpsc;
use gnome_search_provider_common::futures_util::StreamExt;
use gnome_search_provider_common::gio;
use gnome_search_provider_common::gio::glib;
use gnome_search_provider_common::logging::*;
use gnome_search_provider_common::mainloop::*;
use gnome_search_provider_common::matching::*;
use gnome_search_provider_common::zbus;

/// Read paths of all recent projects from the given `reader`.
fn parse_recent_jetbrains_projects<R: Read>(home: &str, reader: R) -> Result<Vec<String>> {
    let element = Element::from_reader(reader)?;
    event!(Level::TRACE, "Finding projects in {:?}", element);

    let projects = element
        .find_all("component")
        .find(|e| {
            e.get_attr("name") == Some("RecentProjectsManager")
                || e.get_attr("name") == Some("RiderRecentProjectsManager")
        })
        .and_then(|comp| {
            comp.find_all("option")
                .find(|e| e.get_attr("name") == Some("additionalInfo"))
        })
        .and_then(|opt| opt.find("map"))
        .map(|map| {
            map.find_all("entry")
                .filter_map(|entry| entry.get_attr("key"))
                .map(|key| key.replace("$USER_HOME$", home))
                .collect()
        })
        .unwrap_or_default();

    event!(
        Level::TRACE,
        "Parsed projects {:?} from {:?}",
        projects,
        element
    );

    Ok(projects)
}

/// Try to read the name of a Jetbrains project from the `name` file of the given project directory.
///
/// Look for a `name` file in the `.idea` sub-directory and return the contents of this file.
fn read_name_from_file<P: AsRef<Path>>(path: P) -> Result<String> {
    let name_file = path.as_ref().join(".idea").join(".name");
    event!(
        Level::TRACE,
        "Trying to read name from {}",
        name_file.display()
    );
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read project name from {}", name_file.display()))?;
    Ok(contents.trim().to_string())
}

/// Get the name of the Jetbrains product at the given path.
///
/// Look for a `name` file in the `.idea` sub-directory; if that file does not exist
/// or cannot be read take the file name of `path`, and ultimately return `None` if
/// the name cannot be determined.
fn get_project_name<P: AsRef<Path>>(path: P) -> Option<String> {
    match read_name_from_file(path.as_ref()) {
        Ok(name) => Some(name),
        Err(error) => {
            event!(
                Level::DEBUG,
                "Failed to read project name from file {:#}; falling back to file name of {}",
                error,
                path.as_ref().display()
            );
            path.as_ref()
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
        }
    }
}

/// A path with an associated version.
#[derive(Debug)]
struct VersionedPath {
    path: PathBuf,
    /// The version as pair of epoch and major version.
    version: (u16, u16),
}

impl VersionedPath {
    /// Extract the version number from the given path.
    ///
    /// Return `None` if the path doesn't contain any valid version.
    #[instrument]
    fn extract_version(path: PathBuf) -> Option<VersionedPath> {
        static RE: OnceCell<Regex> = OnceCell::new();
        let re = RE.get_or_init(|| Regex::new(r"(\d{1,4}).(\d{1,2})").unwrap());
        event!(
            Level::TRACE,
            "Parsing {} with {}",
            path.display(),
            re.as_str()
        );

        let version = path
            .file_name()
            .and_then(OsStr::to_str)
            .and_then(|filename| re.captures(filename))
            .map(|m| (u16::from_str(&m[1]).unwrap(), u16::from_str(&m[2]).unwrap()));
        event!(
            Level::TRACE,
            "Parsing {} with {} -> {:?}",
            path.display(),
            re.as_str(),
            version
        );

        version.map(|version| VersionedPath { path, version })
    }

    /// Get the path out of this versioned path
    fn into_path(self) -> PathBuf {
        self.path
    }
}

/// A location for configuration of a Jetbrains product.
#[derive(Debug)]
struct ConfigLocation<'a> {
    /// The vendor configuration directory.
    vendor_dir: &'a str,
    /// A prefix for configuration directories inside the vendor directory.
    config_prefix: &'a str,
    /// The file name for recent projects
    projects_filename: &'a str,
}

impl ConfigLocation<'_> {
    /// Find the configuration directory of the latest installed product version.
    fn find_config_dir_of_latest_version(&self, config_home: &Path) -> Result<VersionedPath> {
        let vendor_dir = config_home.join(self.vendor_dir);
        let dir = std::fs::read_dir(&vendor_dir)
            .with_context(|| format!("Failed to open directory {}", vendor_dir.display()))?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|entry| {
                if let Some(name) = entry.file_name().and_then(|name| name.to_str()) {
                    name.starts_with(self.config_prefix)
                } else {
                    false
                }
            })
            .filter_map(VersionedPath::extract_version)
            .max_by_key(|p| p.version);
        event!(
            Level::DEBUG,
            "Found config dir {:?} in {}",
            dir,
            config_home.display()
        );
        dir.ok_or_else(|| {
            anyhow!(
                "Failed to find configuration directory in {}",
                config_home.display(),
            )
        })
    }

    /// Find the latest recent projects file.
    #[instrument]
    fn find_latest_recent_projects_file(&self, config_home: &Path) -> Result<PathBuf> {
        let file = self
            .find_config_dir_of_latest_version(config_home)?
            .into_path()
            .join("options")
            .join(self.projects_filename);
        event!(
            Level::TRACE,
            "Using recent projects file at {:?} in {}",
            file,
            config_home.display()
        );
        Ok(file)
    }
}

/// A search provider to expose from this service.
struct ProviderDefinition<'a> {
    /// A human readable label for this provider.
    label: &'a str,
    /// The ID (that is, the filename) of the desktop file of the corresponding app.
    desktop_id: &'a str,
    /// The relative object path to expose this provider at.
    relative_obj_path: &'a str,
    /// The location of the configuration of the corresponding product.
    config: ConfigLocation<'a>,
}

impl ProviderDefinition<'_> {
    /// Gets the full object path for this provider.
    fn objpath(&self) -> String {
        format!(
            "/de/swsnr/searchprovider/jetbrains/{}",
            self.relative_obj_path
        )
    }
}

/// Known search providers.
///
/// For each definition in this array a corresponding provider file must exist in
/// `providers/`; the file must refer to the same `desktop_id` and the same object path.
/// The object path must be unique for each desktop ID, to ensure that this service always
/// launches the right application associated with the search provider.
const PROVIDERS: &[ProviderDefinition] = &[
    ProviderDefinition {
        label: "CLion (toolbox)",
        desktop_id: "jetbrains-clion.desktop",
        relative_obj_path: "toolbox/clion",
        config: ConfigLocation {
            vendor_dir: "JetBrains",
            config_prefix: "CLion",
            projects_filename: "recentProjects.xml",
        },
    },
    ProviderDefinition {
        label: "GoLand (toolbox)",
        desktop_id: "jetbrains-goland.desktop",
        relative_obj_path: "toolbox/goland",
        config: ConfigLocation {
            vendor_dir: "JetBrains",
            config_prefix: "GoLand",
            projects_filename: "recentProjects.xml",
        },
    },
    ProviderDefinition {
        label: "IDEA (toolbox)",
        desktop_id: "jetbrains-idea.desktop",
        relative_obj_path: "toolbox/idea",
        config: ConfigLocation {
            vendor_dir: "JetBrains",
            config_prefix: "IntelliJIdea",
            projects_filename: "recentProjects.xml",
        },
    },
    ProviderDefinition {
        label: "IDEA Community Edition (toolbox)",
        desktop_id: "jetbrains-idea-ce.desktop",
        relative_obj_path: "toolbox/ideace",
        config: ConfigLocation {
            vendor_dir: "JetBrains",
            config_prefix: "IdeaIC",
            projects_filename: "recentProjects.xml",
        },
    },
    ProviderDefinition {
        label: "PHPStorm (toolbox)",
        desktop_id: "jetbrains-phpstorm.desktop",
        relative_obj_path: "toolbox/phpstorm",
        config: ConfigLocation {
            vendor_dir: "JetBrains",
            config_prefix: "PhpStorm",
            projects_filename: "recentProjects.xml",
        },
    },
    ProviderDefinition {
        label: "PyCharm (toolbox)",
        desktop_id: "jetbrains-pycharm.desktop",
        relative_obj_path: "toolbox/pycharm",
        config: ConfigLocation {
            vendor_dir: "JetBrains",
            config_prefix: "PyCharm",
            projects_filename: "recentProjects.xml",
        },
    },
    ProviderDefinition {
        label: "Rider (toolbox)",
        desktop_id: "jetbrains-rider.desktop",
        relative_obj_path: "toolbox/rider",
        config: ConfigLocation {
            vendor_dir: "JetBrains",
            config_prefix: "Rider",
            projects_filename: "recentSolutions.xml",
        },
    },
    ProviderDefinition {
        label: "RubyMine (toolbox)",
        desktop_id: "jetbrains-rubymine.desktop",
        relative_obj_path: "toolbox/rubymine",
        config: ConfigLocation {
            vendor_dir: "JetBrains",
            config_prefix: "RubyMine",
            projects_filename: "recentProjects.xml",
        },
    },
    ProviderDefinition {
        label: "Android Studio (toolbox)",
        desktop_id: "jetbrains-studio.desktop",
        relative_obj_path: "toolbox/studio",
        config: ConfigLocation {
            vendor_dir: "Google",
            config_prefix: "AndroidStudio",
            projects_filename: "recentProjects.xml",
        },
    },
    ProviderDefinition {
        label: "WebStorm (toolbox)",
        desktop_id: "jetbrains-webstorm.desktop",
        relative_obj_path: "toolbox/webstorm",
        config: ConfigLocation {
            vendor_dir: "JetBrains",
            config_prefix: "WebStorm",
            projects_filename: "recentProjects.xml",
        },
    },
];

#[instrument(fields(app_id = %app_id))]
fn read_recent_items(
    config: &ConfigLocation<'_>,
    app_id: &AppId,
) -> Result<IndexMap<String, AppLaunchItem>> {
    event!(Level::INFO, %app_id, "Searching recent projects");
    match config
        .find_latest_recent_projects_file(&glib::user_config_dir())
        .and_then(|projects_file| {
            File::open(&projects_file).with_context(|| {
                format!(
                    "Failed to open recent projects file at {}",
                    projects_file.display()
                )
            })
        }) {
        Ok(mut source) => {
            let home = glib::home_dir();
            let home_s = home
                .to_str()
                .with_context(|| "Failed to convert home directory path to UTF-8 string")?;
            let mut items = IndexMap::new();
            for path in parse_recent_jetbrains_projects(home_s, &mut source)? {
                if let Some(name) = get_project_name(&path) {
                    event!(Level::TRACE, %app_id, "Found project {} at {}", name, path);
                    let id = format!("jetbrains-recent-project-{app_id}-{path}");
                    items.insert(
                        id,
                        AppLaunchItem {
                            name,
                            uri: path.to_string(),
                        },
                    );
                } else {
                    event!(Level::TRACE, %app_id, "Skipping {}, failed to determine project name", path);
                }
            }
            event!(Level::INFO, %app_id, "Found {} recent project(s) for app {}", items.len(), app_id);
            Ok(items)
        }
        Err(error) => {
            event!(Level::DEBUG, %error, "No recent items available: {:#}", error);
            Ok(IndexMap::new())
        }
    }
}

/// The name to request on the bus.
const BUSNAME: &str = "de.swsnr.searchprovider.Jetbrains";

async fn tick(connection: zbus::Connection) {
    loop {
        connection.executor().tick().await
    }
}

async fn get_items(
    app_id: AppId,
    config: &'static ConfigLocation<'static>,
    pool: &glib::ThreadPool,
) -> Result<IndexMap<String, AppLaunchItem>> {
    let id_inner = app_id.clone();
    let span_for_pool = Span::current();
    event!(Level::DEBUG, %app_id, "Spawning task on thread pool to read recent items");
    pool.push_future(move || span_for_pool.in_scope(|| read_recent_items(config, &id_inner)))
        .with_context(|| "Failed to run task on IO thread pool".to_string())?
        .await
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}

/// Handle a single search provider request.
///
/// Handle `request` and return the new list of app items, if any.
#[instrument(skip(pool, items), fields(app_id=%app_id, request=%request.name()))]
async fn handle_search_provider_request(
    app_id: AppId,
    config: &'static ConfigLocation<'static>,
    pool: &glib::ThreadPool,
    items: Option<Arc<IndexMap<String, AppLaunchItem>>>,
    request: AppItemSearchRequest,
) -> Option<Arc<IndexMap<String, AppLaunchItem>>> {
    match request {
        AppItemSearchRequest::Invalidate(_) => {
            if items.is_some() {
                event!(Level::DEBUG, %app_id, "Invalidating cached projects");
            }
            None
        }
        AppItemSearchRequest::GetItems(_, respond_to) => {
            let reply = match items {
                None => {
                    get_items(app_id.clone(), config, pool).in_current_span().await.map_err(|error| {
                        event!(Level::ERROR, %app_id, %error, "Failed to get recent items: {:#}", error);
                        zbus::fdo::Error::Failed(format!("Failed to get recent items: {error}"))
                    }).map(Arc::new)

                }
                Some(ref items) => Ok(Arc::clone(items)),
            };
            let items = reply.as_ref().map(|a| a.clone()).ok();
            // We don't care if the receiver was dropped before we could answer it.
            let _ = respond_to.send(reply);
            items
        }
    }
}

/// Serve search provider requests.
///
/// Loop over requests received from `rx`, and provide the search provider with appropriate
/// responses.
///
/// `pool` is used to spawn blocking IO.
async fn serve_search_provider(
    app_id: AppId,
    config: &'static ConfigLocation<'static>,
    pool: glib::ThreadPool,
    mut rx: mpsc::Receiver<AppItemSearchRequest>,
) {
    let mut items = None;
    loop {
        match rx.next().await {
            None => {
                event!(Level::DEBUG, %app_id, "No more requests from search provider, stopping");
                break;
            }
            Some(request) => {
                let span = request.span().clone();
                items =
                    handle_search_provider_request(app_id.clone(), config, &pool, items, request)
                        .instrument(span)
                        .await;
            }
        }
    }
}

/// The running service.
#[derive(Debug)]
struct Service {
    /// The launch service used to launch applications.
    launch_service: AppLaunchService,
    /// The DBus connection of this service.
    connection: zbus::Connection,
}

/// Starts the DBUS service.
///
/// Connect to the session bus and register a new DBus object for every provider
/// whose underlying app is installed.
///
/// Then register the connection on the Glib main loop and install a callback to
/// handle incoming messages.
async fn start_dbus_service(log_control: LogControl) -> Result<Service> {
    let launch_service = AppLaunchService::new();

    let mut providers = Vec::with_capacity(PROVIDERS.len());
    for provider in PROVIDERS {
        if let Some(gio_app) = gio::DesktopAppInfo::new(provider.desktop_id) {
            event!(Level::INFO, "Found app {}", provider.desktop_id);
            let (tx, rx) = mpsc::channel(8);
            let search_provider =
                AppItemSearchProvider::new(gio_app.into(), launch_service.client(), tx);
            // Move IO to a separate thread pool to avoid blocking the main loop.
            // We use a shared pool to share two threads among all providers.
            let io_pool = glib::ThreadPool::shared(Some(2)).with_context(|| {
                format!(
                    "Failed to create thread pool to read recent projects for app {}",
                    search_provider.app().id()
                )
            })?;
            glib::MainContext::ref_thread_default().spawn(serve_search_provider(
                search_provider.app().id().clone(),
                &provider.config,
                io_pool,
                rx,
            ));
            providers.push((provider.objpath(), search_provider));
        } else {
            event!(
                Level::DEBUG,
                desktop_id = provider.desktop_id,
                "Skipping provider, app not found"
            );
        }
    }

    event!(
        Level::INFO,
        "Registering {} search provider(s) on {}",
        providers.len(),
        BUSNAME
    );

    event!(Level::DEBUG, "Connecting to session bus");
    let connection = zbus::ConnectionBuilder::session()?
        // .serve_at("/org/freedesktop/LogControl1", log_control)?
        // .name(BUSNAME)?
        // We disable the internal executor because we'd like to run the connection
        // exclusively on the glib mainloop, and thus tick it manually (see below).
        .internal_executor(false)
        .build()
        .await
        .with_context(|| "Failed to connect to session bus")?;

    // Manually tick the connection on the glib mainloop to make all code in zbus run on the mainloop.
    glib::MainContext::ref_thread_default().spawn(tick(connection.clone()));

    event!(
        Level::DEBUG,
        "Connected to session bus, registering log control interface"
    );
    connection
        .object_server()
        .at("/org/freedesktop/LogControl1", log_control)
        .await?;

    event!(
        Level::DEBUG,
        "Log control interface registered, registering {} search provider interfaces",
        providers.len()
    );
    for (path, provider) in providers {
        event!(
            Level::DEBUG,
            app_id = %provider.app().id(),
            "Serving search provider at {}",
            path
        );
        connection
            .object_server()
            .at(path.as_str(), provider)
            .await
            .with_context(|| format!("Failed to register search provider at {path}"))?;
    }

    event!(
        Level::DEBUG,
        "All search providers registers, acquiring bus name {}",
        BUSNAME
    );
    connection.request_name(BUSNAME).await?;

    event!(
        Level::INFO,
        "Acquired name {}, serving search providers",
        BUSNAME
    );
    Ok(Service {
        launch_service,
        connection,
    })
}

fn app() -> clap::Command {
    use clap::*;
    command!()
        .dont_collapse_args_in_usage(true)
        .term_width(80)
        .after_help(
            "\
Set $RUST_LOG to control the log level",
        )
        .arg(
            Arg::new("providers")
                .long("providers")
                .action(ArgAction::SetTrue)
                .help("List all providers"),
        )
}

fn main() {
    let matches = app().get_matches();
    if matches.get_flag("providers") {
        let mut labels: Vec<&'static str> = PROVIDERS.iter().map(|p| p.label).collect();
        labels.sort_unstable();
        for label in labels {
            println!("{label}")
        }
    } else {
        let log_control = setup_logging_for_service();

        event!(
            Level::INFO,
            "Started {} version: {}",
            env!("CARGO_BIN_NAME"),
            env!("CARGO_PKG_VERSION")
        );

        match glib::MainContext::ref_thread_default().block_on(start_dbus_service(log_control)) {
            Ok(service) => {
                let _ = service.launch_service.start(
                    service.connection,
                    SystemdScopeSettings {
                        prefix: concat!("app-", env!("CARGO_BIN_NAME")).to_string(),
                        started_by: env!("CARGO_BIN_NAME").to_string(),
                        documentation: vec![env!("CARGO_PKG_HOMEPAGE").to_string()],
                    },
                );
                create_main_loop(&glib::MainContext::ref_thread_default()).run();
            }
            Err(error) => {
                event!(Level::ERROR, %error, "Failed to start DBus server: {:#}", error);
                std::process::exit(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use similar_asserts::assert_eq;

    use super::*;

    #[test]
    fn verify_app() {
        app().debug_assert();
    }

    #[test]
    fn versioned_path_extract() {
        let path = glib::home_dir()
            .join(".config")
            .join("JetBrains")
            .join("IdeaIC2021.1");
        let versioned_path = VersionedPath::extract_version(path).unwrap();
        assert_eq!(versioned_path.version, (2021, 1))
    }

    #[test]
    fn read_recent_projects() {
        let data: &[u8] = include_bytes!("tests/recentProjects.xml");
        let home = glib::home_dir();
        let items = parse_recent_jetbrains_projects(home.to_str().unwrap(), data).unwrap();

        assert_eq!(
            items,
            vec![
                home.join("Code")
                    .join("gh")
                    .join("mdcat")
                    .to_string_lossy()
                    .to_string(),
                home.join("Code")
                    .join("gh")
                    .join("gnome-search-providers-jetbrains")
                    .to_string_lossy()
                    .to_string()
            ]
        )
    }

    #[test]
    fn read_recent_solutions() {
        let data: &[u8] = include_bytes!("tests/recentSolutions.xml");
        let home = glib::home_dir();
        let items = parse_recent_jetbrains_projects(home.to_str().unwrap(), data).unwrap();

        assert_eq!(
            items,
            vec![
                home.join("Code")
                    .join("gh")
                    .join("mdcat")
                    .to_string_lossy()
                    .to_string(),
                home.join("Code")
                    .join("gh")
                    .join("gnome-search-providers-jetbrains")
                    .to_string_lossy()
                    .to_string()
            ]
        )
    }

    mod providers {
        use crate::{BUSNAME, PROVIDERS};
        use anyhow::{anyhow, Context, Result};
        use std::collections::HashSet;
        use std::path::Path;

        struct ProviderFile {
            desktop_id: String,
            object_path: String,
            bus_name: String,
            version: String,
        }

        fn load_all_provider_files() -> Result<Vec<ProviderFile>> {
            let mut providers = Vec::new();
            let ini_files = globwalk::GlobWalkerBuilder::new(
                Path::new(env!("CARGO_MANIFEST_DIR")).join("providers"),
                "*.ini",
            )
            .build()
            .unwrap();
            for entry in ini_files {
                let filepath = entry.unwrap().into_path();
                let mut ini = configparser::ini::Ini::new();
                ini.load(&filepath).map_err(|s| {
                    anyhow!("Failed to parse ini file at {}: {}", filepath.display(), s)
                })?;
                let provider = ProviderFile {
                    desktop_id: ini
                        .get("Shell Search Provider", "DesktopId")
                        .with_context(|| format!("DesktopId missing in {}", &filepath.display()))?,
                    object_path: ini
                        .get("Shell Search Provider", "ObjectPath")
                        .with_context(|| {
                            format!("ObjectPath missing in {}", &filepath.display())
                        })?,
                    bus_name: ini
                        .get("Shell Search Provider", "BusName")
                        .with_context(|| format!("BusName missing in {}", &filepath.display()))?,
                    version: ini
                        .get("Shell Search Provider", "Version")
                        .with_context(|| format!("Version missing in {}", &filepath.display()))?,
                };
                providers.push(provider);
            }

            Ok(providers)
        }

        #[test]
        fn all_providers_have_a_correct_ini_file() {
            let provider_files = load_all_provider_files().unwrap();
            for provider in PROVIDERS {
                let provider_file = provider_files
                    .iter()
                    .find(|p| p.desktop_id == provider.desktop_id);
                assert!(
                    provider_file.is_some(),
                    "Provider INI missing for provider {} with desktop ID {}",
                    provider.label,
                    provider.desktop_id
                );

                assert_eq!(provider_file.unwrap().object_path, provider.objpath());
                assert_eq!(provider_file.unwrap().bus_name, BUSNAME);
                assert_eq!(provider_file.unwrap().version, "2");
            }
        }

        #[test]
        fn no_extra_ini_files_without_providers() {
            let provider_files = load_all_provider_files().unwrap();
            assert_eq!(PROVIDERS.len(), provider_files.len());
        }

        #[test]
        fn desktop_ids_are_unique() {
            let mut ids = HashSet::new();
            for provider in PROVIDERS {
                ids.insert(provider.desktop_id);
            }
            assert_eq!(PROVIDERS.len(), ids.len());
        }

        #[test]
        fn dbus_paths_are_unique() {
            let mut paths = HashSet::new();
            for provider in PROVIDERS {
                paths.insert(provider.objpath());
            }
            assert_eq!(PROVIDERS.len(), paths.len());
        }
    }
}
