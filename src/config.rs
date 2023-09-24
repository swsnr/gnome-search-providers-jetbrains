// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Jetbrains configuration helpers.

use anyhow::{anyhow, Context, Result};
use glib::once_cell::sync::OnceCell;
use regex::Regex;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use tracing::{event, instrument, Level};

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
pub struct ConfigLocation<'a> {
    /// The vendor configuration directory.
    pub vendor_dir: &'a str,
    /// A prefix for configuration directories inside the vendor directory.
    pub config_prefix: &'a str,
    /// The file name for recent projects
    pub projects_filename: &'a str,
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
    pub fn find_latest_recent_projects_file(&self, config_home: &Path) -> Result<PathBuf> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use similar_asserts::assert_eq;

    #[test]
    fn versioned_path_extract() {
        let path = glib::home_dir()
            .join(".config")
            .join("JetBrains")
            .join("IdeaIC2021.1");
        let versioned_path = VersionedPath::extract_version(path).unwrap();
        assert_eq!(versioned_path.version, (2021, 1))
    }
}
