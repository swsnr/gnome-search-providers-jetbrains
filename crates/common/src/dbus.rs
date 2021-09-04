// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! DBus helpers for search providers.

use std::fmt::Debug;

use log::trace;
use thiserror::Error;
use zbus::fdo::{DBusProxy, RequestNameFlags, RequestNameReply};
use zbus::Connection;

/// An error occurred when acquiring a bus name.
#[derive(Error, Debug)]
pub enum AcquireNameError {
    /// Access to the bus failed.
    #[error("Access to bus failed")]
    BusError(#[from] zbus::Error),
    /// The request to the bus name failed.
    #[error("Request to acquire name failed")]
    FdoError(#[from] zbus::fdo::Error),
    /// The call succeeded but the bus refused to hand us the requested name.
    #[error("Failed to acquire bus name {0}: {1:?}")]
    RequestNameRejected(String, RequestNameReply),
}

/// Acquire a name on the given connection.
pub fn acquire_bus_name<S: AsRef<str>>(
    connection: &Connection,
    name: S,
) -> Result<(), AcquireNameError> {
    let flags = RequestNameFlags::DoNotQueue;
    trace!("RequestName({}, {:?})", name.as_ref(), flags);
    let result = DBusProxy::new(connection)?.request_name(name.as_ref(), flags.into());
    trace!(
        "RequestName({}, {:?}) -> {:?}",
        name.as_ref(),
        flags,
        result
    );
    let reply = result?;
    if reply == RequestNameReply::PrimaryOwner {
        Ok(())
    } else {
        Err(AcquireNameError::RequestNameRejected(
            name.as_ref().to_string(),
            reply,
        ))
    }
}
