// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Logging setup

use tracing_subscriber::filter::{EnvFilter, LevelFilter};
use tracing_subscriber::prelude::*;
use tracing_subscriber::Registry;

/// Setup logging for a service
///
/// If the output is connected to journald log directly to systemd journal;
/// otherwise log to standard output.
pub fn setup_logging_for_service() {
    glib::log_set_default_handler(glib::rust_log_handler);
    tracing_log::LogTracer::init().unwrap();

    let use_journal = libsystemd::logging::connected_to_journal();
    let (journald, stdout) = if use_journal {
        (
            Some(
                tracing_journald::Layer::new()
                    .unwrap()
                    .with_field_prefix(None),
            ),
            None,
        )
    } else {
        (None, Some(tracing_subscriber::fmt::layer().pretty()))
    };

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::default().add_directive(LevelFilter::INFO.into()));
    let subscriber = Registry::default()
        .with(stdout)
        .with(journald)
        .with(env_filter);

    tracing::subscriber::set_global_default(subscriber).unwrap();
}
