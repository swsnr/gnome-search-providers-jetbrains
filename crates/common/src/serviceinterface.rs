// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! The global DBus control interface for a running search providers service.

use crate::app::{AppId, AppItemSearchRequest};
use futures_channel::mpsc::Sender;
use futures_util::future::join_all;
use futures_util::{FutureExt, SinkExt};
use tracing::{event, instrument, Level, Span};
use zbus::dbus_interface;

/// A global control interface for a search providers service.
#[derive(Debug)]
pub struct ServiceInterface {
    search_services: Vec<(AppId, Sender<AppItemSearchRequest>)>,
}

impl ServiceInterface {
    /// Create a new global service interface.
    pub fn new(search_services: Vec<(AppId, Sender<AppItemSearchRequest>)>) -> Self {
        Self { search_services }
    }
}

/// Extra methods for search providers.
#[dbus_interface(name = "de.swsnr.searchprovider.SearchProviders")]
impl ServiceInterface {
    /// Refresh all items in the search provider.
    #[instrument(skip(self))]
    pub async fn refresh_all(&mut self) -> zbus::fdo::Result<()> {
        event!(Level::DEBUG, "Refreshing all search provider items");
        let requests = self.search_services.iter_mut().map(|(app_id, service)| {
            service
                .send(AppItemSearchRequest::RefreshItems(Span::current()))
                .map(|result| (app_id.clone(), result))
        });
        let results = join_all(requests).await;
        let is_failed = results
            .iter()
            .map(|(app_id, result)| {
                if let Err(error) = result {
                    event!(Level::ERROR, %app_id, "Failed to refresh items of {}: {}", app_id, error);
                }
                result
            })
            .any(|r| r.is_err());
        if is_failed {
            Err(zbus::fdo::Error::Failed(
                "Failed to refresh some items".to_string(),
            ))
        } else {
            Ok(())
        }
    }
}
