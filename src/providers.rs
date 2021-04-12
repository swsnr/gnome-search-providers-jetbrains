// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::jetbrains::config::ConfigLocation;

pub struct ProviderDefinition<'a> {
    pub desktop_id: &'a str,
    pub relative_obj_path: &'a str,
    pub config: ConfigLocation<'a>,
}

pub const PROVIDERS: [ProviderDefinition; 10] = [
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
