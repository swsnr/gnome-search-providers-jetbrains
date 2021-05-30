// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Common utilities and library for Gnome Search Providers.

#![deny(warnings, missing_docs, clippy::all)]

pub mod app;
pub mod dbus;
pub mod log;
pub mod mainloop;
pub mod matching;
pub mod systemd;

/// Re-exports.
pub mod export {
    pub use zbus;
}
