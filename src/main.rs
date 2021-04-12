// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

#![deny(warnings, missing_docs, clippy::all)]

//! Gnome search provider for Jetbrains products

mod gnome;
mod jetbrains;
mod providers;

fn main() {
    for provider in &providers::PROVIDERS {
        if let Some(path) = provider
            .config
            .find_latest_recent_projects_file(&dirs::config_dir().unwrap())
        {
            println!("Path {}", path.display());
        }
    }
}
