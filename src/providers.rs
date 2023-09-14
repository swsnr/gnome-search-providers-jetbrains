// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Provider definitions.

use crate::config::ConfigLocation;

/// A search provider to expose from this service.
pub struct ProviderDefinition<'a> {
    /// A human readable label for this provider.
    pub label: &'a str,
    /// The ID (that is, the filename) of the desktop file of the corresponding app.
    pub desktop_id: &'a str,
    /// The relative object path to expose this provider at.
    pub relative_obj_path: &'a str,
    /// The location of the configuration of the corresponding product.
    pub config: ConfigLocation<'a>,
}

impl ProviderDefinition<'_> {
    /// Gets the full object path for this provider.
    pub fn objpath(&self) -> String {
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
pub const PROVIDERS: &[ProviderDefinition] = &[
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
        label: "RustRover (toolbox)",
        desktop_id: "jetbrains-rustrover.desktop",
        relative_obj_path: "toolbox/rustrover",
        config: ConfigLocation {
            vendor_dir: "JetBrains",
            config_prefix: "RustRover",
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

#[cfg(test)]
mod tests {
    use similar_asserts::assert_eq;
    use std::collections::HashSet;
    use std::fs::File;
    use std::io::{BufRead, BufReader};
    use std::path::Path;

    use anyhow::{anyhow, Context, Result};

    use crate::{BUSNAME, PROVIDERS};

    struct ProviderFile {
        desktop_id: String,
        object_path: String,
        bus_name: String,
        version: String,
    }

    fn load_all_provider_files() -> Result<Vec<ProviderFile>> {
        let mut providers = Vec::new();
        let provider_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("providers");
        for entry in std::fs::read_dir(provider_dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().unwrap() != "ini" {
                continue;
            }
            let mut ini = configparser::ini::Ini::new();
            ini.load(&path)
                .map_err(|s| anyhow!("Failed to parse ini file at {}: {}", path.display(), s))?;
            let provider = ProviderFile {
                desktop_id: ini
                    .get("Shell Search Provider", "DesktopId")
                    .with_context(|| format!("DesktopId missing in {}", &path.display()))?,
                object_path: ini
                    .get("Shell Search Provider", "ObjectPath")
                    .with_context(|| format!("ObjectPath missing in {}", &path.display()))?,
                bus_name: ini
                    .get("Shell Search Provider", "BusName")
                    .with_context(|| format!("BusName missing in {}", &path.display()))?,
                version: ini
                    .get("Shell Search Provider", "Version")
                    .with_context(|| format!("Version missing in {}", &path.display()))?,
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

    #[test]
    fn all_providers_are_in_readme() {
        let readme = Path::new(env!("CARGO_MANIFEST_DIR")).join("README.md");
        let lines: Vec<String> = BufReader::new(File::open(readme).unwrap())
            .lines()
            .map(|l| l.unwrap())
            .skip_while(|l| l != "Supports")
            .skip(2)
            .take_while(|l| !l.is_empty())
            .collect();
        let mut expected_lines: Vec<String> =
            PROVIDERS.iter().map(|p| format!("- {}", p.label)).collect();
        expected_lines.sort();
        assert_eq!(lines, expected_lines);
    }
}
