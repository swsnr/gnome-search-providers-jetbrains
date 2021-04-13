// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

#![deny(warnings, missing_docs, clippy::all)]

//! Gnome search provider for Jetbrains products

use lazy_static::lazy_static;
use regex::Regex;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use zbus::dbus_interface;
use zbus::export::zvariant::Value;

/// A path with an associated version.
#[derive(Debug)]
struct VersionedPath {
    path: PathBuf,
    version: (u16, u16),
}

impl VersionedPath {
    /// Extract the version number from the given path.
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
            .map(|path| {
                VersionedPath::extract_version(path).unwrap_or(|| {
                    panic!(
                        "Failed to extract version from {} even though it matched a glob path",
                        path.display()
                    )
                })
            })
            .max_by_key(|p| p.version)
    }

    /// Find the latest recent projects file
    fn find_latest_recent_projects_file(&self, config_home: &Path) -> Option<PathBuf> {
        self.find_config_dir_of_latest_version(config_home)
            .map(|p| p.into_path())
            .map(|p| p.join("options").join(self.projects_filename))
            .filter(|p| p.is_file())
    }
}

struct RecentProject {
    name: String,
    path: PathBuf,
}

fn load_projects(recent_projects_file: &Path) -> Vec<RecentProject> {
    unimplemented!()
}

struct ProviderDefinition<'a> {
    desktop_id: &'a str,
    relative_obj_path: &'a str,
    config: ConfigLocation<'a>,
}

const PROVIDERS: [ProviderDefinition; 10] = [
    ProviderDefinition {
        desktop_id: "jetbrains-clion.desktop",
        relative_obj_path: "toolbox/clion",
        config: ConfigLocation {
            vendor_dir: "JetBrains",
            config_glob: "CLion*",
            projects_filename: "recentProjects.xml",
        },
    },
    ProviderDefinition {
        desktop_id: "jetbrains-goland.desktop",
        relative_obj_path: "toolbox/goland",
        config: ConfigLocation {
            vendor_dir: "JetBrains",
            config_glob: "GoLand*",
            projects_filename: "recentProjects.xml",
        },
    },
    ProviderDefinition {
        desktop_id: "jetbrains-idea.desktop",
        relative_obj_path: "toolbox/idea",
        config: ConfigLocation {
            vendor_dir: "JetBrains",
            config_glob: "IntelliJIdea*",
            projects_filename: "recentProjects.xml",
        },
    },
    ProviderDefinition {
        desktop_id: "jetbrains-idea-ce.desktop",
        relative_obj_path: "toolbox/ideace",
        config: ConfigLocation {
            vendor_dir: "JetBrains",
            config_glob: "IdeaIC*",
            projects_filename: "recentProjects.xml",
        },
    },
    ProviderDefinition {
        desktop_id: "jetbrains-phpstorm.desktop",
        relative_obj_path: "toolbox/phpstorm",
        config: ConfigLocation {
            vendor_dir: "JetBrains",
            config_glob: "PhpStorm*",
            projects_filename: "recentProjects.xml",
        },
    },
    ProviderDefinition {
        desktop_id: "jetbrains-pycharm.desktop",
        relative_obj_path: "toolbox/pycharm",
        config: ConfigLocation {
            vendor_dir: "JetBrains",
            config_glob: "PyCharm*",
            projects_filename: "recentProjects.xml",
        },
    },
    ProviderDefinition {
        desktop_id: "jetbrains-rider.desktop",
        relative_obj_path: "toolbox/rider",
        config: ConfigLocation {
            vendor_dir: "JetBrains",
            config_glob: "Rider*",
            projects_filename: "recentSolutions.xml",
        },
    },
    ProviderDefinition {
        desktop_id: "jetbrains-rubymine.desktop",
        relative_obj_path: "toolbox/rubymine",
        config: ConfigLocation {
            vendor_dir: "JetBrains",
            config_glob: "RubyMine*",
            projects_filename: "recentProjects.xml",
        },
    },
    ProviderDefinition {
        desktop_id: "jetbrains-studio.desktop",
        relative_obj_path: "toolbox/studio",
        config: ConfigLocation {
            vendor_dir: "Google",
            config_glob: "AndroidStudio*",
            projects_filename: "recentProjects.xml",
        },
    },
    ProviderDefinition {
        desktop_id: "jetbrains-webstorm.desktop",
        relative_obj_path: "toolbox/webstorm",
        config: ConfigLocation {
            vendor_dir: "JetBrains",
            config_glob: "WebStorm*",
            projects_filename: "recentProjects.xml",
        },
    },
];

struct DBusSearchProvider {
}

#[dbus_interface(name = "org.zbus.MyGreeter1")]
impl DBusSearchProvider {
    fn get_initial_result_set(&mut self, terms: Vec<String>) -> Vec<String> {
        unimplemented!()
    }

    fn get_subsearch_result_set(
        &self,
        previous_results: Vec<String>,
        terms: Vec<String>,
    ) -> Vec<String> {
        unimplemented!()
    }

    fn get_result_metas(&mut self, results: Vec<String>) -> Vec<HashMap<String, Value>> {
        unimplemented!()
    }

    fn activate_result(&mut self, id: String, terms: Vec<String>, timestamp: u32) {
        unimplemented!()
    }

    fn launch_search(&mut self, terms: Vec<String>, timestamp: u32) {
        unimplemented!()
    }
}

fn main() {
    for provider in &PROVIDERS {
        if let Some(path) = provider
            .config
            .find_latest_recent_projects_file(&dirs::config_dir().unwrap())
        {
            println!("Path {}", path.display());
        }
    }
}
