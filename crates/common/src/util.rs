// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Misc utilities.

/// Where to log to.
#[derive(Debug, PartialEq, Copy, Clone)]
pub enum LogDestination {
    /// Standard output.
    Stdout,
    /// The systemd journal.
    Journal,
}

/// Setup logging for the given `destination`.
///
/// The maximum level is set to "info", unless the `$LOG_DEBUG` environment variable is set in which case it's "debug".
pub fn setup_logging(destination: LogDestination) {
    match destination {
        LogDestination::Journal => {
            systemd::JournalLog::init().unwrap();
            log::set_max_level(log::LevelFilter::Info);
        }
        LogDestination::Stdout => {
            env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
                .init();
        }
    }

    if std::env::var_os("LOG_DEBUG").is_some() {
        log::set_max_level(log::LevelFilter::Debug)
    }
}
