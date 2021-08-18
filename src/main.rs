// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

#![deny(warnings, missing_docs, clippy::all)]

//! Gnome search provider for Jetbrains products

use std::ffi::OsStr;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{Context, Result};
use elementtree::Element;
use lazy_static::lazy_static;
use regex::Regex;
use slog::*;
use std::convert::TryFrom;

use gnome_search_provider_common::app::*;
use gnome_search_provider_common::dbus::acquire_bus_name;
use gnome_search_provider_common::export::zbus;
use gnome_search_provider_common::export::zbus::export::names::WellKnownName;
use gnome_search_provider_common::log::*;
use gnome_search_provider_common::mainloop::run_dbus_loop;
use gnome_search_provider_common::matching::*;
use gnome_search_provider_common::systemd::Systemd1Manager;

/// A path with an associated version.
#[derive(Debug)]
struct VersionedPath {
    path: PathBuf,
    /// The version as pair of epoch and major version.
    version: (u16, u16),
}

/// Read paths of all recent projects from the given `reader`.
fn read_recent_jetbrains_projects<R: Read>(reader: R) -> Result<Vec<String>> {
    let element = Element::from_reader(reader)?;
    let home = dirs::home_dir()
        .with_context(|| "$HOME directory required")?
        .into_os_string()
        .into_string()
        .ok()
        .with_context(|| "$HOME not a valid UTF-8 string")?;

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

        let version = path
            .file_name()
            .and_then(OsStr::to_str)
            .and_then(|filename| RE.captures(filename))
            .map(|m| (u16::from_str(&m[1]).unwrap(), u16::from_str(&m[2]).unwrap()));

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
    /// A glob for configuration directories inside the vendor directory.
    config_glob: &'a str,
    /// The file name for recent projects
    projects_filename: &'a str,
}

impl ConfigLocation<'_> {
    /// Find the configuration directory of the latest installed product version.
    fn find_config_dir_of_latest_version(&self, config_home: &Path) -> Option<VersionedPath> {
        let vendor_dir = config_home.join(self.vendor_dir);
        globwalk::GlobWalkerBuilder::new(vendor_dir, self.config_glob)
            .build()
            .expect("Failed to build glob pattern")
            .filter_map(Result::ok)
            .map(globwalk::DirEntry::into_path)
            .filter_map(VersionedPath::extract_version)
            .max_by_key(|p| p.version)
    }

    /// Find the latest recent projects file.
    fn find_latest_recent_projects_file(&self, config_home: &Path) -> Option<PathBuf> {
        self.find_config_dir_of_latest_version(config_home)
            .map(|p| p.into_path())
            .map(|p| p.join("options").join(self.projects_filename))
            .filter(|p| p.is_file())
    }
}

/// Get the name of the Jetbrains product at the given path.
///
/// Look for a `name` file in the `.idea` sub-directory; if that file does not exist
/// or cannot be read take the file name of `path`, and ultimately return `None` if
/// the name cannot be determined.
fn get_project_name<P: AsRef<Path>>(path: P) -> Option<String> {
    File::open(path.as_ref().join(".idea").join(".name"))
        .and_then(|mut source| {
            let mut buffer = String::new();
            source.read_to_string(&mut buffer)?;
            Ok(buffer)
        })
        .ok()
        .or_else(|| {
            path.as_ref()
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
        })
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
            config_glob: "CLion*",
            projects_filename: "recentProjects.xml",
        },
    },
    ProviderDefinition {
        label: "GoLand (toolbox)",
        desktop_id: "jetbrains-goland.desktop",
        relative_obj_path: "toolbox/goland",
        config: ConfigLocation {
            vendor_dir: "JetBrains",
            config_glob: "GoLand*",
            projects_filename: "recentProjects.xml",
        },
    },
    ProviderDefinition {
        label: "IDEA (toolbox)",
        desktop_id: "jetbrains-idea.desktop",
        relative_obj_path: "toolbox/idea",
        config: ConfigLocation {
            vendor_dir: "JetBrains",
            config_glob: "IntelliJIdea*",
            projects_filename: "recentProjects.xml",
        },
    },
    ProviderDefinition {
        label: "IDEA Community Edition (toolbox)",
        desktop_id: "jetbrains-idea-ce.desktop",
        relative_obj_path: "toolbox/ideace",
        config: ConfigLocation {
            vendor_dir: "JetBrains",
            config_glob: "IdeaIC*",
            projects_filename: "recentProjects.xml",
        },
    },
    ProviderDefinition {
        label: "PHPStorm (toolbox)",
        desktop_id: "jetbrains-phpstorm.desktop",
        relative_obj_path: "toolbox/phpstorm",
        config: ConfigLocation {
            vendor_dir: "JetBrains",
            config_glob: "PhpStorm*",
            projects_filename: "recentProjects.xml",
        },
    },
    ProviderDefinition {
        label: "PyCharm (toolbox)",
        desktop_id: "jetbrains-pycharm.desktop",
        relative_obj_path: "toolbox/pycharm",
        config: ConfigLocation {
            vendor_dir: "JetBrains",
            config_glob: "PyCharm*",
            projects_filename: "recentProjects.xml",
        },
    },
    ProviderDefinition {
        label: "Rider (toolbox)",
        desktop_id: "jetbrains-rider.desktop",
        relative_obj_path: "toolbox/rider",
        config: ConfigLocation {
            vendor_dir: "JetBrains",
            config_glob: "Rider*",
            projects_filename: "recentSolutions.xml",
        },
    },
    ProviderDefinition {
        label: "RubyMine (toolbox)",
        desktop_id: "jetbrains-rubymine.desktop",
        relative_obj_path: "toolbox/rubymine",
        config: ConfigLocation {
            vendor_dir: "JetBrains",
            config_glob: "RubyMine*",
            projects_filename: "recentProjects.xml",
        },
    },
    ProviderDefinition {
        label: "Android Studio (toolbox)",
        desktop_id: "jetbrains-studio.desktop",
        relative_obj_path: "toolbox/studio",
        config: ConfigLocation {
            vendor_dir: "Google",
            config_glob: "AndroidStudio*",
            projects_filename: "recentProjects.xml",
        },
    },
    ProviderDefinition {
        label: "WebStorm (toolbox)",
        desktop_id: "jetbrains-webstorm.desktop",
        relative_obj_path: "toolbox/webstorm",
        config: ConfigLocation {
            vendor_dir: "JetBrains",
            config_glob: "WebStorm*",
            projects_filename: "recentProjects.xml",
        },
    },
];

struct JetbrainsProjectsSource<'a> {
    log: Logger,
    app_id: String,
    /// Where to look for the configuration and the list of recent projects.
    config: &'a ConfigLocation<'a>,
}

impl<'a> ItemsSource<AppLaunchItem> for JetbrainsProjectsSource<'a> {
    type Err = anyhow::Error;

    fn find_recent_items(&self) -> Result<IdMap<AppLaunchItem>, Self::Err> {
        info!(self.log, "Searching recent projects for {}", &self.app_id);
        let mut items = IndexMap::new();
        let config_home = dirs::config_dir().unwrap();
        if let Some(projects_file) = self.config.find_latest_recent_projects_file(&config_home) {
            debug!(
                self.log,
                "Found configuration file for {} at {}",
                &self.app_id,
                projects_file.display()
            );
            for path in read_recent_jetbrains_projects(File::open(projects_file)?)? {
                trace!(
                    self.log,
                    "Found project directory for {} at {}",
                    &self.app_id,
                    &path
                );
                if let Some(name) = get_project_name(&path) {
                    let id = format!("jetbrains-recent-project-{}-{}", self.app_id, path);
                    trace!(
                        self.log,
                        "Found project {} (id: {}) for {} at {}",
                        &name,
                        &id,
                        &self.app_id,
                        &path
                    );
                    items.insert(
                        id,
                        AppLaunchItem {
                            name,
                            target: AppLaunchTarget::File(path),
                        },
                    );
                }
            }
        };
        info!(
            self.log,
            "Found {} project(s) for {}",
            items.len(),
            &self.app_id
        );
        Ok(items)
    }
}

/// The name to request on the bus.
const BUSNAME: &str = "de.swsnr.searchprovider.Jetbrains";

fn register_search_providers(
    root_log: &Logger,
    connection: &zbus::Connection,
    object_server: &mut zbus::ObjectServer,
) -> Result<()> {
    for provider in PROVIDERS {
        if let Some(app) = gio::DesktopAppInfo::new(provider.desktop_id) {
            let log = root_log.new(
                o!("desktop_app_id" => provider.desktop_id, "provider_objpath" => provider.objpath()),
            );
            info!(
                log,
                "Registering provider for {} at {}",
                provider.desktop_id,
                provider.objpath(),
            );
            let dbus_provider = AppItemSearchProvider::new(
                log.clone(),
                app,
                JetbrainsProjectsSource {
                    log: log.clone(),
                    app_id: provider.desktop_id.to_string(),
                    config: &provider.config,
                },
                Systemd1Manager::new(log.clone(), connection)
                    .with_context(|| "Failed to access systemd manager via DBUS")?,
                SystemdScopeSettings {
                    prefix: concat!("app-", env!("CARGO_BIN_NAME")).to_string(),
                    started_by: env!("CARGO_BIN_NAME").to_string(),
                    documentation: vec![env!("CARGO_PKG_HOMEPAGE").to_string()],
                },
            );
            object_server.at(provider.objpath().as_str(), dbus_provider)?;
        }
    }
    Ok(())
}

/// Starts the DBUS service.
///
/// Connect to the session bus and register a new DBus object for every provider
/// whose underlying app is installed.
///
/// Then register the connection on the Glib main loop and install a callback to
/// handle incoming messages.
fn start_dbus_service(root_log: &Logger) -> Result<()> {
    let log = root_log.new(o!("busname" => BUSNAME));

    info!(log, "Connecting to session bus");
    let connection =
        zbus::Connection::session().with_context(|| "Failed to connect to session bus")?;

    debug!(log, "Creating object server");
    let mut object_server = zbus::ObjectServer::new(&connection);

    debug!(log, "Registering all search providers on object server");
    register_search_providers(&log, &connection, &mut object_server)?;
    info!(log, "All providers registered, acquiring {}", BUSNAME);
    acquire_bus_name(&log, &connection, WellKnownName::try_from(BUSNAME).unwrap())?;
    info!(
        log,
        "Acquired name {}, starting service main loop to handle DBus messages", BUSNAME
    );

    run_dbus_loop(log.clone(), connection, move |message| match object_server
        .dispatch_message(message)
    {
        Ok(true) => debug!(log, "Message dispatched to object server: {:?} ", message),
        Ok(false) => warn!(log, "Message not handled by object server: {:?}", message),
        Err(error) => error!(
            log,
            "Failed to dispatch message {:?} on object server: {}", message, error
        ),
    })
    .map_err(Into::into)
}

fn main() {
    use clap::*;

    let app = app_from_crate!()
        .setting(AppSettings::UnifiedHelpMessage)
        .setting(AppSettings::DontCollapseArgsInUsage)
        .setting(AppSettings::DeriveDisplayOrder)
        .set_term_width(80)
        .after_help(
            "\
Set $RUST_LOG to control the log level",
        )
        .arg(
            Arg::with_name("providers")
                .long("--providers")
                .help("List all providers"),
        );
    let matches = app.get_matches();
    if matches.is_present("providers") {
        let mut labels: Vec<&'static str> = PROVIDERS.iter().map(|p| p.label).collect();
        labels.sort_unstable();
        for label in labels {
            println!("{}", label)
        }
    } else {
        let logger = create_service_logger(o!("version" => env!("CARGO_PKG_VERSION")));
        slog_stdlog::init().unwrap();
        let _guard = slog_scope::set_global_logger(logger.clone());

        info!(
            logger,
            "Started {} version: {}",
            env!("CARGO_BIN_NAME"),
            env!("CARGO_PKG_VERSION")
        );

        if let Err(err) = start_dbus_service(&logger) {
            error!(logger, "Main loop error: {:#}", err);
            std::process::exit(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn versioned_path_extract() {
        let path = dirs::home_dir()
            .expect("Must have homedir for test")
            .join(".config")
            .join("JetBrains")
            .join("IdeaIC2021.1");
        let versioned_path = VersionedPath::extract_version(path).unwrap();
        assert_eq!(versioned_path.version, (2021, 1))
    }

    #[test]
    fn read_recent_projects() {
        let data: &[u8] = include_bytes!("tests/recentProjects.xml");
        let home = dirs::home_dir().unwrap();
        let items = read_recent_jetbrains_projects(data).unwrap();

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
        let home = dirs::home_dir().unwrap();
        let items = read_recent_jetbrains_projects(data).unwrap();

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
