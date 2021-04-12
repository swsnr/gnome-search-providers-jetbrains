// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use lazy_static::lazy_static;
use regex::Regex;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::str::FromStr;

/// A path with an associated version.
#[derive(Debug)]
pub struct VersionedPath {
    path: PathBuf,
    version: (u16, u16),
}

impl VersionedPath {
    pub fn from_path(path: PathBuf) -> Option<VersionedPath> {
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
    pub fn into_path(self) -> PathBuf {
        self.path
    }
}

/// A location for configuration of a Jetbrains product.
#[derive(Debug)]
pub struct ConfigLocation<'a> {
    /// The vendor configuration directory.
    pub vendor_dir: &'a str,
    /// A glob for configuration directories inside the vendor directory.
    pub config_glob: &'a str,
    /// The file name for recent projects
    pub projects_filename: &'a str,
}

impl ConfigLocation<'_> {
    /// Find the configuration directory of the latest installed product version.
    pub fn find_config_dir_of_latest_version(&self, config_home: &Path) -> Option<VersionedPath> {
        let vendor_dir = config_home.join(self.vendor_dir);
        globwalk::GlobWalkerBuilder::new(vendor_dir, self.config_glob)
            .build()
            .expect("Failed to build glob pattern")
            .filter_map(Result::ok)
            .map(globwalk::DirEntry::into_path)
            .filter_map(VersionedPath::from_path)
            .max_by_key(|p| p.version)
    }

    /// Find the latest recent projects file
    pub fn find_latest_recent_projects_file(&self, config_home: &Path) -> Option<PathBuf> {
        self.find_config_dir_of_latest_version(config_home)
            .map(|p| p.into_path())
            .map(|p| p.join("options").join(self.projects_filename))
            .filter(|p| p.is_file())
    }
}
