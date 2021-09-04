// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! DBus helpers for search providers.

use crate::export::futures_util::StreamExt;
use log::{error, trace, warn};
use zbus::azync::Connection;
use zbus::export::names::WellKnownName;
use zbus::fdo::{AsyncDBusProxy, RequestNameFlags, RequestNameReply};

/// Acquire a name on the given connection.
pub async fn request_name_exclusive(
    connection: &Connection,
    name: WellKnownName<'_>,
) -> Result<RequestNameReply, zbus::fdo::Error> {
    let flags = RequestNameFlags::DoNotQueue | RequestNameFlags::ReplaceExisting;
    trace!("RequestName({}, {:?})", name.as_str(), flags);
    AsyncDBusProxy::new(connection)
        .await?
        .request_name(name, flags)
        .await
}

/// Run an object server on the given connection.
///
/// Continuously polls the connection for new messagesand dispatches them to `server`.
pub async fn run_server(mut connection: zbus::azync::Connection, mut server: zbus::ObjectServer) {
    while let Some(result) = connection.next().await {
        match result {
            Ok(message) => match server.dispatch_message(&message) {
                Ok(true) => trace!("Message dispatched to object server: {:?} ", message),
                Ok(false) => warn!("Message not handled by object server: {:?}", message),
                Err(error) => error!(
                    "Failed to dispatch message {:?} on object server: {}",
                    message, error
                ),
            },
            Err(error) => error!("Failed to receive message from bus connection: {:?}", error),
        }
    }
}
