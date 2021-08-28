// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Mainloop utilities for dbus serch providers.

use std::os::unix::io::AsRawFd;

use slog::{debug, error, o, Logger};
use thiserror::Error;

use gio::glib;
use glib::source::SourceId;
use slog::warn;
use zbus::ObjectServer;

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
///
/// `on_message` is not required to be `Send` but the calling thread needs to
/// own the main context to make sure that `on_message` remains on the main thread.
pub fn source_add_connection_local<F: FnMut(&zbus::Message) + 'static>(
    log: Logger,
    connection: zbus::Connection,
    mut on_message: F,
) -> SourceId {
    let fd = connection.as_raw_fd();
    glib::source::unix_fd_add_local(
        fd,
        glib::IOCondition::IN | glib::IOCondition::PRI,
        move |_, condition| {
            debug!(
                log,
                "Connection {file_descriptor} entered IO condition {:?}",
                condition,
                file_descriptor = fd
            );
            match connection.receive_message() {
                Ok(message) => on_message(&message),
                Err(err) => error!(
                    log,
                    "Failed to process message on connection {}: {:#}", fd, err,
                ),
            }
            glib::Continue(true)
        },
    )
}

/// Handle messages received on the given `connection` with `on_message`.
pub fn run_dbus_loop<F: FnMut(&zbus::Message) + 'static>(
    log: Logger,
    connection: zbus::Connection,
    on_message: F,
) -> Result<(), MainLoopError> {
    let context = glib::MainContext::default();
    let guard = context
        .acquire()
        .map_err(|_| MainLoopError::FailedToAcquireContext)?;
    let mainloop = glib::MainLoop::new(Some(&context), false);

    source_add_connection_local(log.new(o!()), connection, on_message);

    glib::source::unix_signal_add(
        libc::SIGTERM,
        glib::clone!(@strong mainloop, @strong log =>  move || {
            debug!(log, "Received {signal}, quitting mainloop", signal = "SIGTERM");
            mainloop.quit();
            glib::Continue(false)
        }),
    );

    glib::source::unix_signal_add(
        libc::SIGINT,
        glib::clone!(@strong mainloop, @strong log =>  move || {
            debug!(log, "Received {signal}, quitting mainloop", signal = "SIGINT");
            mainloop.quit();
            glib::Continue(false)
        }),
    );

    mainloop.run();
    // We no longer require the main context.
    drop(guard);
    Ok(())
}

/// Dispatch messages received on the given `connection` to the given `object_server`.
pub fn run_object_server_loop(
    log: Logger,
    connection: zbus::Connection,
    mut object_server: ObjectServer,
) -> Result<(), MainLoopError> {
    run_dbus_loop(log.clone(), connection, move |message| match object_server
        .dispatch_message(message)
    {
        Ok(true) => debug!(log, "Message dispatched to object server: {:?} ", message),
        Ok(false) => warn!(log, "Message not handled by object server: {:?}", message),
        Err(error) => error!(
            log,
            "Failed to dispatch message {:?} on object server: {}", message, error
        ),
    })
}
