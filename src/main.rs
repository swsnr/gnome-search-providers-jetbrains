// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

#![deny(warnings, missing_docs, clippy::all)]

//! Gnome search provider for Jetbrains products

use anyhow::{Context, Result};
use tracing::{event, Level};

mod config;
mod launchservice;
mod logcontrol;
mod logging;
mod providers;
mod reload;
mod searchprovider;
mod systemd;

use crate::launchservice::{App, AppLaunchService, SystemdScopeSettings};
use providers::*;
use reload::*;
use searchprovider::*;

/// The name to request on the bus.
const BUSNAME: &str = "de.swsnr.searchprovider.Jetbrains";

async fn tick(connection: zbus::Connection) {
    loop {
        connection.executor().tick().await
    }
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
        let log_control = logging::setup_logging_for_service();

        event!(
            Level::INFO,
            "Started {} version: {}",
            env!("CARGO_BIN_NAME"),
            env!("CARGO_PKG_VERSION")
        );

        let launch_service = AppLaunchService::new();
        event!(
            Level::DEBUG,
            "Connecting to session bus, registering interfaces for search providers, and acquiring {}",
            BUSNAME
        );

        let main_context = glib::MainContext::ref_thread_default();

        // Connect to DBus and register all our objects for search providers.
        let connection = main_context.block_on(async {
            PROVIDERS
                .iter()
                .filter_map(|provider| {
                    gio::DesktopAppInfo::new(provider.desktop_id).map(|gio_app| {
                        event!(Level::INFO, "Found app {}", provider.desktop_id);
                        let mut search_provider = JetbrainsProductSearchProvider::new(
                            App::from(gio_app),
                            launch_service.client(),
                            &provider.config,
                        );
                        let _ = search_provider.reload_items();
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
                .serve_at("/org/freedesktop/LogControl1", log_control)?
                .name(BUSNAME)?
                .build()
                .await
                .with_context(|| "Failed to connect to session bus")
        })?;

        // Manually tick the connection on the glib mainloop to make all code in zbus run on the mainloop.
        main_context.spawn(tick(connection.clone()));

        event!(
            Level::INFO,
            "Acquired name {}, serving search providers",
            BUSNAME
        );

        let _ = launch_service.start(
            connection,
            SystemdScopeSettings {
                prefix: concat!("app-", env!("CARGO_BIN_NAME")).to_string(),
                started_by: env!("CARGO_BIN_NAME").to_string(),
                documentation: vec![env!("CARGO_PKG_HOMEPAGE").to_string()],
            },
        );
        let mainloop = glib::MainLoop::new(Some(&main_context), false);

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
