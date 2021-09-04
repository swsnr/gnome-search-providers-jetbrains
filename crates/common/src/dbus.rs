// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! DBus helpers for search providers.

use log::trace;
use zbus::fdo::{DBusProxy, RequestNameFlags, RequestNameReply};
use zbus::Connection;

/// Acquire a name on the given connection.
pub fn acquire_bus_name<S: AsRef<str>>(
    connection: &Connection,
    name: S,
) -> Result<RequestNameReply, zbus::fdo::Error> {
    let flags = RequestNameFlags::DoNotQueue | RequestNameFlags::ReplaceExisting;
    trace!("RequestName({}, {:?})", name.as_ref(), flags);
    let result = DBusProxy::new(connection)?.request_name(name.as_ref(), flags.into());
    trace!(
        "RequestName({}, {:?}) -> {:?}",
        name.as_ref(),
        flags,
        result
    );
    result
}
