// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Logging setup

/// Setup logging for the given `destination`.
///
/// If `$JOURNAL_STREAM` is set and non-empty directly log to the systemd journal,
/// as per [systemd.exec](https://www.freedesktop.org/software/systemd/man/systemd.exec.html#Environment%20Variables%20in%20Spawned%20Processes).
///
/// The maximum level is set to "info", unless the `$LOG_DEBUG` environment variable is set in which case it's "debug".
pub fn setup_logging_for_service() {
    if crate::systemd::connected_to_journal() {
        systemd::JournalLog::init().unwrap();
        log::set_max_level(log::LevelFilter::Info);
    } else {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    }

    if std::env::var_os("LOG_DEBUG").is_some() {
        log::set_max_level(log::LevelFilter::Debug)
    }
}
