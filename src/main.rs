// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

#![deny(warnings, missing_docs, clippy::all)]

//! Gnome search provider for Jetbrains products

use std::convert::TryFrom;
use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use elementtree::Element;
use lazy_static::lazy_static;
use log::{debug, error, info, trace};
use regex::Regex;

use gnome_search_provider_common::app::*;
use gnome_search_provider_common::futures_channel;
use gnome_search_provider_common::gio;
use gnome_search_provider_common::gio::glib;
use gnome_search_provider_common::gio::prelude::*;
use gnome_search_provider_common::log::*;
use gnome_search_provider_common::mainloop::*;
use gnome_search_provider_common::matching::*;
use gnome_search_provider_common::source::*;
use gnome_search_provider_common::zbus;
use gnome_search_provider_common::zbus::names::WellKnownName;

/// A path with an associated version.
#[derive(Debug)]
struct VersionedPath {
    path: PathBuf,
    /// The version as pair of epoch and major version.
    version: (u16, u16),
}

/// Read paths of all recent projects from the given `reader`.
fn parse_recent_jetbrains_projects<R: Read>(reader: R) -> Result<Vec<String>> {
    let element = Element::from_reader(reader)?;
    let home = glib::home_dir()
        .into_os_string()
        .into_string()
        .ok()
        .with_context(|| "$HOME not a valid UTF-8 string")?;

    trace!("Finding projects in {:?}", element);

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
                .map(|key| key.replace("$USER_HOME$", &home))
                .collect()
        })
        .unwrap_or_default();

    trace!("Parsed projects {:?} from {:?}", projects, element);

    Ok(projects)
}

impl VersionedPath {
    /// Extract the version number from the given path.
    ///
    /// Return `None` if the path doesn't contain any valid version.
    fn extract_version(path: PathBuf) -> Option<VersionedPath> {
        lazy_static! {
            static ref RE: Regex = Regex::new(r"(\d{1,4}).(\d{1,2})").unwrap();
        }

        trace!("Parsing {} with {}", path.display(), RE.as_str());

        let version = path
            .file_name()
            .and_then(OsStr::to_str)
            .and_then(|filename| RE.captures(filename))
            .map(|m| (u16::from_str(&m[1]).unwrap(), u16::from_str(&m[2]).unwrap()));

        trace!(
            "Parsing {} with {} -> {:?}",
            path.display(),
            RE.as_str(),
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
    async fn find_config_dir_of_latest_version(&self, config_home: &Path) -> Result<VersionedPath> {
        let vendor_dir = gio::File::for_path(config_home.join(self.vendor_dir));
        let files: Vec<gio::FileInfo> = vendor_dir
            .enumerate_children_async_future(
                &gio::FILE_ATTRIBUTE_STANDARD_NAME,
                gio::FileQueryInfoFlags::NONE,
                glib::PRIORITY_DEFAULT,
            )
            .await
            .with_context(|| format!("Failed to enumerate children of {}", vendor_dir.uri()))?
            .next_files_async_future(i32::MAX, glib::PRIORITY_DEFAULT)
            .await
            .with_context(|| format!("Failed to get children of {}", vendor_dir.uri()))?;

        let dir = files
            .iter()
            .filter_map(|f| {
                f.name()
                    .file_name()
                    .and_then(|n| n.to_str())
                    .filter(|name| name.starts_with(self.config_prefix))
                    .map(|_| vendor_dir.path().unwrap().join(f.name()))
            })
            .filter_map(VersionedPath::extract_version)
            .max_by_key(|p| p.version);

        debug!("Found config dir {:?} in {}", dir, config_home.display());
        dir.ok_or_else(|| {
            anyhow!(
                "Failed to find configuration directory in {}",
                config_home.display(),
            )
        })
    }

    /// Find the latest recent projects file.
    async fn find_latest_recent_projects_file(&self, config_home: &Path) -> Result<PathBuf> {
        let file = self
            .find_config_dir_of_latest_version(config_home)
            .await?
            .into_path()
            .join("options")
            .join(self.projects_filename);
        debug!(
            "Using recent projects file at {:?} in {}",
            file,
            config_home.display()
        );
        Ok(file)
    }
}

/// Try to read the name of a Jetbrains project from the `name` file of the given project directory.
///
/// Look for a `name` file in the `.idea` sub-directory and return the contents of this file.
async fn read_name_from_file<P: AsRef<Path>>(path: P) -> Result<String> {
    let name_file = gio::File::for_path(path.as_ref().join(".idea").join(".name"));
    trace!("Trying to read name from {}", name_file.uri());
    let (data, _) = name_file
        .load_contents_async_future()
        .await
        .with_context(|| format!("Failed to read project name from {}", name_file.uri()))?;
    Ok(String::from_utf8_lossy(&data).trim().to_string())
}

/// Get the name of the Jetbrains product at the given path.
///
/// Look for a `name` file in the `.idea` sub-directory; if that file does not exist
/// or cannot be read take the file name of `path`, and ultimately return `None` if
/// the name cannot be determined.
async fn get_project_name<P: AsRef<Path>>(path: P) -> Option<String> {
    match read_name_from_file(path.as_ref()).await {
        Ok(name) => Some(name),
        Err(error) => {
            debug!("Failed to read project name from file: {:#}", error);
            debug!(
                "Falling back to file name of {} as project name",
                path.as_ref().display()
            );
            path.as_ref()
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
        }
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

struct JetbrainsProjectsSource<'a> {
    app_id: AppId,
    /// Where to look for the configuration and the list of recent projects.
    config: &'a ConfigLocation<'a>,
}

async fn read_recent_items(
    config: &ConfigLocation<'_>,
    app_id: AppId,
) -> Result<IdMap<AppLaunchItem>> {
    info!("Searching recent projects for {}", app_id);
    let mut items = IndexMap::new();
    let projects_file = gio::File::for_path(
        config
            .find_latest_recent_projects_file(&glib::user_config_dir())
            .await?,
    );

    let (data, _) = projects_file
        .load_contents_async_future()
        .await
        .with_context(|| {
            format!(
                "Failed to read recent projects contents from {}",
                projects_file.uri()
            )
        })?;

    for path in parse_recent_jetbrains_projects(&*data)? {
        if let Some(name) = get_project_name(&path).await {
            trace!("Found project {} at {} for {}", name, path, app_id);
            let id = format!("jetbrains-recent-project-{}-{}", app_id, path);
            items.insert(
                id,
                AppLaunchItem {
                    name,
                    uri: path.to_string(),
                },
            );
        } else {
            trace!("Skipping {}, failed to determine project name", path);
        }
    }
    info!("Found {} project(s) for {}", items.len(), app_id);
    Ok(items)
}

#[async_trait]
impl AsyncItemsSource<AppLaunchItem> for JetbrainsProjectsSource<'static> {
    type Err = anyhow::Error;

    async fn find_recent_items(&self) -> Result<IdMap<AppLaunchItem>, Self::Err> {
        let (send, recv) = futures_channel::oneshot::channel();
        let app_id = self.app_id.clone();
        let config = self.config;
        // Move to the main thread and then asynchronously read recent items through Gio,
        // and get them sent back to us via a oneshot channel.
        glib::MainContext::default().invoke(move || {
            glib::MainContext::default().spawn_local(async move {
                let result = read_recent_items(config, app_id).await;
                send.send(result).unwrap();
            });
        });
        recv.await.unwrap()
    }
}

/// The name to request on the bus.
const BUSNAME: &str = "de.swsnr.searchprovider.Jetbrains";

async fn register_search_providers(
    connection: &zbus::Connection,
    launch_service: &AppLaunchService,
) -> Result<()> {
    for provider in PROVIDERS {
        if let Some(app) = gio::DesktopAppInfo::new(provider.desktop_id) {
            info!(
                "Registering provider for {} at {}",
                provider.desktop_id,
                provider.objpath()
            );
            let dbus_provider = AppItemSearchProvider::new(
                app.into(),
                JetbrainsProjectsSource {
                    app_id: provider.desktop_id.into(),
                    config: &provider.config,
                },
                launch_service.client(),
            );
            connection
                .object_server()
                .at(provider.objpath().as_str(), dbus_provider)
                .await?;
        }
    }
    Ok(())
}

async fn tick(connection: zbus::Connection) {
    loop {
        connection.executor().tick().await
    }
}

/// The running service.
#[derive(Debug)]
struct Service {
    /// The launch service used to launch applications.
    #[allow(unused)]
    launch_service: AppLaunchService,
}

/// Starts the DBUS service.
///
/// Connect to the session bus and register a new DBus object for every provider
/// whose underlying app is installed.
///
/// Then register the connection on the Glib main loop and install a callback to
/// handle incoming messages.
async fn start_dbus_service() -> Result<Service> {
    let connection = zbus::ConnectionBuilder::session()?
        .internal_executor(false)
        .build()
        .await
        .with_context(|| "Failed to connect to session bus")?;

    glib::MainContext::ref_thread_default().spawn(tick(connection.clone()));

    info!("Registering all search providers");
    let launch_service = AppLaunchService::new(
        &glib::MainContext::ref_thread_default(),
        connection.clone(),
        SystemdScopeSettings {
            prefix: concat!("app-", env!("CARGO_BIN_NAME")).to_string(),
            started_by: env!("CARGO_BIN_NAME").to_string(),
            documentation: vec![env!("CARGO_PKG_HOMEPAGE").to_string()],
        },
    );
    register_search_providers(&connection, &launch_service).await?;

    info!("All providers registered, acquiring {}", BUSNAME);
    connection
        .request_name(WellKnownName::try_from(BUSNAME).unwrap())
        .await
        .with_context(|| format!("Failed to request {}", BUSNAME))?;

    info!("Acquired name {}, serving search providers", BUSNAME);
    Ok(Service { launch_service })
}

fn app() -> clap::App<'static> {
    use clap::*;
    app_from_crate!()
        .setting(AppSettings::DontCollapseArgsInUsage)
        .setting(AppSettings::DeriveDisplayOrder)
        .term_width(80)
        .after_help(
            "\
Set $RUST_LOG to control the log level",
        )
        .arg(
            Arg::new("providers")
                .long("--providers")
                .help("List all providers"),
        )
}

fn main() {
    let matches = app().get_matches();
    if matches.is_present("providers") {
        let mut labels: Vec<&'static str> = PROVIDERS.iter().map(|p| p.label).collect();
        labels.sort_unstable();
        for label in labels {
            println!("{}", label)
        }
    } else {
        setup_logging_for_service(env!("CARGO_PKG_VERSION"));

        info!(
            "Started {} version: {}",
            env!("CARGO_BIN_NAME"),
            env!("CARGO_PKG_VERSION")
        );

        trace!("Acquire main context");
        let context = glib::MainContext::default();
        context.push_thread_default();

        match context.block_on(start_dbus_service()) {
            Ok(service) => {
                create_main_loop(&context).run();
                // Make sure that the entire service is alive until the mainloop exists
                drop(service);
            }
            Err(error) => {
                error!("Failed to start DBus server: {:#}", error);
                std::process::exit(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

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
        let items = parse_recent_jetbrains_projects(data).unwrap();

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
        let items = parse_recent_jetbrains_projects(data).unwrap();

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
        use anyhow::{Context, Result};
        use ini::Ini;
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
                let ini = Ini::load_from_file(&filepath).with_context(|| {
                    format!("Failed to parse ini file at {}", filepath.display())
                })?;
                let provider = ProviderFile {
                    desktop_id: ini
                        .get_from(Some("Shell Search Provider"), "DesktopId")
                        .with_context(|| format!("DesktopId missing in {}", &filepath.display()))?
                        .to_string(),
                    object_path: ini
                        .get_from(Some("Shell Search Provider"), "ObjectPath")
                        .with_context(|| format!("ObjectPath missing in {}", &filepath.display()))?
                        .to_string(),
                    bus_name: ini
                        .get_from(Some("Shell Search Provider"), "BusName")
                        .with_context(|| format!("BusName missing in {}", &filepath.display()))?
                        .to_string(),
                    version: ini
                        .get_from(Some("Shell Search Provider"), "Version")
                        .with_context(|| format!("Version missing in {}", &filepath.display()))?
                        .to_string(),
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
