// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Utilities for working with DBus.

use log::{trace, warn};
use zbus::fdo::{DBusProxy, RequestNameFlags, RequestNameReply};
use zbus::names::WellKnownName;
use zbus::Connection;

/// Request DBus `name` exclusively.
pub async fn request_name_exclusive(
    connection: &Connection,
    name: WellKnownName<'_>,
) -> Result<(), zbus::fdo::Error> {
    let flags = RequestNameFlags::DoNotQueue.into();
    trace!("RequestName({}, {:?})", name.as_str(), flags);
    let result = DBusProxy::new(connection)
        .await?
        .request_name(name.clone(), flags)
        .await;
    trace!(
        "RequestName({}, {:?}) -> {:?}",
        name.as_str(),
        flags,
        result
    );
    let reply = result?;
    match reply {
        RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner => Ok(()),
        RequestNameReply::Exists => Err(zbus::fdo::Error::AddressInUse(format!(
            "Name {} already exists on bus",
            name
        ))),
        RequestNameReply::InQueue => {
            warn!("Inconsistent reply: Broker put process in queue for {} even though queuing was not requested", name);
            Err(zbus::fdo::Error::ZBus(zbus::Error::InvalidReply))
        }
    }
}
