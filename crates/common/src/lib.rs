// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Common utilities and library for Gnome Search Providers.

#![deny(warnings, missing_docs, clippy::all)]

pub mod dbus;
pub mod matching;

pub use matching::fs::RecentFileSystemItem;
pub use matching::{find_matching_items, IdMap, IndexMap, ItemsSource, ScoreMatchable};

pub use zbus;

/// A recent item for a search provider.
pub trait RecentItem {
    /// The name of a recent item.
    fn name(&self) -> &str;

    /// The description.
    fn description(&self) -> &str;

    /// Get the URI to open this item.
    fn uri(&self) -> &str;
}
