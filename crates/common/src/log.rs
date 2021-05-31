// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Logging setup

use slog::{Drain, Logger, OwnedKV, SendSyncRefUnwindSafeKV};
use slog_journald::JournaldDrain;

/// Create a logger for a systemd service.
///
/// If the current process is connected to the systemd journal log create a
/// logger which directly logs to the systemd journal to retain structured attributes.
///
/// Otherwise it falls back to terminal output on standard error.
pub fn create_service_logger<T>(options: OwnedKV<T>) -> Logger
where
    T: SendSyncRefUnwindSafeKV + 'static,
{
    if crate::systemd::connected_to_journal() {
        slog::Logger::root(
            slog_envlogger::new(JournaldDrain.ignore_res()).fuse(),
            options,
        )
    } else {
        let stderr = slog_term::PlainSyncDecorator::new(std::io::stderr());
        slog::Logger::root(
            slog_envlogger::new(slog_term::FullFormat::new(stderr).build()).fuse(),
            options,
        )
    }
}
