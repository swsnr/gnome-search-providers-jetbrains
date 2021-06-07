// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Logging setup

use env_logger::filter::Filter;
use log::{Log, Metadata, Record};
use systemd_journal_logger::{connected_to_journal, JournalLog};

struct FilteredLog<T: Log> {
    filter: Filter,
    log: T,
}

impl<T: Log> Log for FilteredLog<T> {
    fn enabled(&self, metadata: &Metadata) -> bool {
        self.filter.enabled(metadata) && self.log.enabled(metadata)
    }

    fn log(&self, record: &Record) {
        if self.filter.matches(record) {
            self.log.log(record);
        }
    }

    fn flush(&self) {
        self.log.flush();
    }
}

/// Setup logging for the given `destination`.
///
/// If `$JOURNAL_STREAM` is set and non-empty directly log to the systemd journal,
/// as per [systemd.exec](https://www.freedesktop.org/software/systemd/man/systemd.exec.html#Environment%20Variables%20in%20Spawned%20Processes).
///
/// The maximum level is set to "info", unless the `$LOG_DEBUG` environment variable is set in which case it's "debug".
pub fn setup_logging_for_service<S>(version: S)
where
    S: AsRef<str> + Send + Sync + 'static,
{
    let default_filter = "info";
    if connected_to_journal() {
        let filter = env_logger::filter::Builder::new()
            .parse(
                std::env::var(env_logger::DEFAULT_FILTER_ENV)
                    .as_ref()
                    .map_or(default_filter, String::as_str),
            )
            .build();
        let log = JournalLog::with_extra_fields(vec![("VERSION", version)]);
        log::set_boxed_logger(Box::new(FilteredLog { filter, log })).unwrap();
        // When logging to systemd log panics as well, to make them appear in a structured way in logs
        log_panics::init();
    } else {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    }
}
