// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Logging setup

use env_logger::filter::Filter;
use log::{Log, Metadata, Record};
use systemd_journal_logger::*;

/// A logger which is filtered.
pub struct FilteredLog<T: Log> {
    log: T,
    filter: Filter,
}

impl<T: Log> FilteredLog<T> {
    /// Create a new filtered logger.
    pub fn new(filter: Filter, log: T) -> Self {
        Self { log, filter }
    }
}

impl<T: Log> Log for FilteredLog<T> {
    /// Whether this logger is enabled for the given `metadata`.
    ///
    /// Returns true if the filter allows the given metadata and
    /// if the underlying logger is enabled for the given metadata.
    fn enabled(&self, metadata: &Metadata) -> bool {
        self.filter.enabled(metadata) && self.log.enabled(metadata)
    }

    /// Log the given `record`.
    ///
    /// Check if the filter matches the given `record`; if so log
    /// the record with the underlying logger, otherwise do nothing.
    fn log(&self, record: &Record) {
        if self.filter.matches(record) {
            self.log.log(record)
        }
    }

    /// Flush the underlying logger.
    fn flush(&self) {
        self.log.flush()
    }
}

/// Setup logging for the given `destination`.
///
/// If `$JOURNAL_STREAM` is set and non-empty directly log to the systemd journal,
/// as per [systemd.exec](https://www.freedesktop.org/software/systemd/man/systemd.exec.html#Environment%20Variables%20in%20Spawned%20Processes).
///
/// The maximum level is set to "info", unless the `$LOG_DEBUG` environment variable is set in which case it's "debug".
pub fn setup_logging_for_service(version: &'static str) {
    glib::log_set_default_handler(glib::rust_log_handler);
    let default_filter = "info";
    let env = env_logger::Env::default().default_filter_or(default_filter);
    if connected_to_journal() {
        let journal = JournalLog::with_extra_fields(vec![("VERSION", version)]);
        // We can't use env_logger::Env because for some mysterious reason `.get_filter` is not exported :|
        let filter = env_logger::filter::Builder::new()
            .parse(
                std::env::var(env_logger::DEFAULT_FILTER_ENV)
                    .as_ref()
                    .map_or(default_filter, |f| f),
            )
            .build();
        let max_level = filter.filter();
        log::set_boxed_logger(Box::new(FilteredLog {
            log: journal,
            filter,
        }))
        .unwrap();
        log::set_max_level(max_level);
    } else {
        env_logger::Builder::from_env(env).init();
    }
}
