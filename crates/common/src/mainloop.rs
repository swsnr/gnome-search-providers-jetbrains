// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Mainloop utilities for dbus serch providers.

use std::os::unix::io::AsRawFd;

use log::{debug, error};
use thiserror::Error;

use glib::source::SourceId;

/// An error occurred while starting the main loop.
#[derive(Error, Debug)]
pub enum MainLoopError {
    /// Failed to acquire the main Glib context.
    #[error("Failed to acquire context")]
    FailedToAcquireContext,
}

/// Add `connection` as source to the default Glib mainloop.
///
/// Invoke `on_message` for every message received from `connection`.
pub fn source_add_connection_local<F: FnMut(zbus::Message) + 'static>(
    connection: zbus::Connection,
    mut on_message: F,
) -> SourceId {
    glib::source::unix_fd_add_local(
        connection.as_raw_fd(),
        glib::IOCondition::IN | glib::IOCondition::PRI,
        move |_, condition| {
            debug!("Connection entered IO condition {:?}", condition);
            match connection.receive_message() {
                Ok(message) => on_message(message),
                Err(err) => error!("Failed to process message: {:#}", err),
            }
            glib::Continue(true)
        },
    )
}

/// Connect to session bus, acquire the given name on the bus, and start handling messages.
pub fn run_dbus_loop<F: FnMut(zbus::Message) + 'static>(
    connection: zbus::Connection,
    on_message: F,
) -> Result<(), MainLoopError> {
    let context = glib::MainContext::default();
    if !context.acquire() {
        Err(MainLoopError::FailedToAcquireContext)
    } else {
        let mainloop = glib::MainLoop::new(Some(&context), false);

        source_add_connection_local(connection, on_message);

        glib::source::unix_signal_add(libc::SIGTERM, {
            let l = mainloop.clone();
            move || {
                debug!("Terminated, quitting mainloop");
                l.quit();
                glib::Continue(false)
            }
        });

        glib::source::unix_signal_add(libc::SIGINT, {
            let l = mainloop.clone();
            move || {
                debug!("Interrupted, quitting mainloop");
                l.quit();
                glib::Continue(false)
            }
        });

        mainloop.run();
        Ok(())
    }
}
