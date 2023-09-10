// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Extensions for search providers.

use std::fmt::Debug;

use futures_channel::mpsc;
use futures_util::SinkExt;
use tracing::{event, instrument, Instrument, Level, Span};
use zbus::dbus_interface;

use crate::app::{AppId, AppItemSearchRequest};

/// Extension methods for search providers.
#[derive(Debug)]
pub struct SearchProviderExtensions {
    app_id: AppId,
    sender: mpsc::Sender<AppItemSearchRequest>,
}

impl SearchProviderExtensions {
    /// Create a new refresh service.
    ///
    /// `sender` is used to talk to the search service implementation.
    ///
    /// `app_id` denotes the application the search service references, for logging purposes.
    pub fn new(app_id: AppId, sender: mpsc::Sender<AppItemSearchRequest>) -> Self {
        Self { app_id, sender }
    }
}

/// Extra methods for search providers.
#[dbus_interface(name = "de.swsnr.searchprovider.SearchProvider")]
impl SearchProviderExtensions {
    /// Refresh all items in the search provider.
    #[instrument(skip(self), fields(app_id = %self.app_id))]
    pub async fn refresh(&mut self) -> zbus::fdo::Result<()> {
        event!(Level::DEBUG, app_id=%self.app_id, "Refreshing items of app {}", self.app_id);
        self.sender
            .send(AppItemSearchRequest::RefreshItems(Span::current()))
            .in_current_span()
            .await
            .map_err(|error| {
                event!(
                    Level::ERROR,
                    %error,
                    app_id=%self.app_id,
                    "Failed to send request to refresh search items in app {}: {}", self.app_id, error
                );
                zbus::fdo::Error::Failed(format!(
                    "Failed to refresh search items for {}",
                    self.app_id
                ))
            })?;
        Ok(())
    }
}
