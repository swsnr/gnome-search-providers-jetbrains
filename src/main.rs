// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

#![deny(warnings, missing_docs, clippy::all)]
#![forbid(unsafe_code)]

//! Gnome search provider for Jetbrains products

use std::fs::File;
use std::os::fd::AsFd;
use std::os::linux::fs::MetadataExt;

use anyhow::{Context, Result};
use tracing::{event, Level};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{fmt, EnvFilter, Registry};

use providers::*;
use reload::*;
use searchprovider::*;

mod config;
mod providers;
mod reload;
mod searchprovider;
mod systemd;

/// The name to request on the bus.
const BUSNAME: &str = "de.swsnr.searchprovider.Jetbrains";

async fn tick(connection: zbus::Connection) {
    loop {
        connection.executor().tick().await
    }
}

/// Check whether this process is directly connected to the systemd journal.
///
/// We inspect `$JOURNAL_STREAM` and compare it against the device and inode numbers of
/// stderr; see `systemd.exec(5)` for details.
fn connected_to_journal() -> bool {
    let var_os = std::env::var_os("JOURNAL_STREAM");
    // TODO: We could perhaps do better, i.e. without duplicating the FD
    let fd_text = std::io::stderr()
        .as_fd()
        .try_clone_to_owned()
        .map(File::from)
        .and_then(|f| f.metadata())
        .ok()
        .map(|m| format!("{}:{}", m.st_dev(), m.st_ino()).into());
    var_os.as_ref().map(|os| os.to_string_lossy()) == fd_text
}

fn app() -> clap::Command {
    use clap::*;
    command!()
        .dont_collapse_args_in_usage(true)
        .term_width(80)
        .after_help(
            "\
Set $RUST_LOG to control the log level",
        )
        .arg(
            Arg::new("providers")
                .long("providers")
                .action(ArgAction::SetTrue)
                .help("List all providers"),
        )
}

fn main() -> Result<()> {
    let matches = app().get_matches();
    if matches.get_flag("providers") {
        let mut labels: Vec<&'static str> = PROVIDERS.iter().map(|p| p.label).collect();
        labels.sort_unstable();
        for label in labels {
            println!("{label}")
        }
        Ok(())
    } else {
        // Setup tracing: If we're connected to systemd, directly log to the journal, otherwise log nicely to the TTY.
        let subscriber = Registry::default().with(
            EnvFilter::try_from_default_env()
                .or_else(|_| EnvFilter::try_new("info"))
                .unwrap(),
        );
        if connected_to_journal() {
            let journal = tracing_journald::Layer::new()
                .with_context(|| "Failed to contact journald for logging".to_string())?;
            tracing::subscriber::set_global_default(subscriber.with(journal)).unwrap();
        } else {
            tracing::subscriber::set_global_default(subscriber.with(fmt::layer().pretty()))
                .unwrap();
        }
        // Direct glib to rust log, and…
        glib::log_set_default_handler(glib::rust_log_handler);
        // …rust log to tracing.
        tracing_log::LogTracer::init().unwrap();

        event!(
            Level::INFO,
            "Started {} version: {}",
            env!("CARGO_BIN_NAME"),
            env!("CARGO_PKG_VERSION")
        );

        event!(
            Level::DEBUG,
            "Connecting to session bus, registering interfaces for search providers, and acquiring {}",
            BUSNAME
        );

        // Connect to DBus and register all our objects for search providers.
        let connection = glib::MainContext::default().block_on(async {
            PROVIDERS
                .iter()
                .filter_map(|provider| {
                    gio::DesktopAppInfo::new(provider.desktop_id).map(|gio_app| {
                        event!(Level::INFO, "Found app {}", provider.desktop_id);
                        let mut search_provider = JetbrainsProductSearchProvider::new(
                            App::from(gio_app),
                            &provider.config,
                        );
                        let _ = search_provider.reload_recent_projects();
                        (provider.objpath(), search_provider)
                    })
                })
                .try_fold(
                    // We disable the internal executor because we'd like to run the connection
                    // exclusively on the glib mainloop, and thus tick it manually (see below).
                    zbus::ConnectionBuilder::session()?.internal_executor(false),
                    |builder, (path, provider)| {
                        event!(
                            Level::DEBUG,
                            app_id = %provider.app().id(),
                            "Serving search provider for {} at {}",
                            provider.app().id(),
                            &path
                        );
                        builder.serve_at(path, provider)
                    },
                )?
                .serve_at("/", ReloadAll)?
                .name(BUSNAME)?
                .build()
                .await
                .with_context(|| "Failed to connect to session bus")
        })?;

        // Manually tick the connection on the glib mainloop to make all code in zbus run on the mainloop.
        glib::MainContext::default().spawn(tick(connection.clone()));

        event!(
            Level::INFO,
            "Acquired name {}, serving search providers",
            BUSNAME
        );

        let mainloop = glib::MainLoop::new(None, false);

        // Quit our mainloop on SIGTERM and SIGINT
        glib::source::unix_signal_add(
            libc::SIGTERM,
            glib::clone!(@strong mainloop =>  move || {
                event!(Level::DEBUG, "Terminated, quitting mainloop");
                mainloop.quit();
                glib::ControlFlow::Break
            }),
        );
        glib::source::unix_signal_add(
            libc::SIGINT,
            glib::clone!(@strong mainloop =>  move || {
                event!(Level::DEBUG, "Interrupted, quitting mainloop");
                mainloop.quit();
                glib::ControlFlow::Break
            }),
        );

        mainloop.run();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_app() {
        app().debug_assert();
    }
}
