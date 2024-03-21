// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Reload all recent projects across all exposed provider interfaces.

use crate::providers::PROVIDERS;
use crate::searchprovider::JetbrainsProductSearchProvider;
use tracing::{event, instrument, Level};
use zbus::{interface, ObjectServer};

#[derive(Debug)]
pub struct ReloadAll;

#[interface(name = "de.swsnr.searchprovider.ReloadAll")]
impl ReloadAll {
    /// Reload all recent projects in all registered search providers..
    #[instrument(skip(self, server))]
    pub async fn reload_all(
        &self,
        #[zbus(object_server)] server: &ObjectServer,
    ) -> zbus::fdo::Result<()> {
        event!(
            Level::DEBUG,
            "Reloading recent projects of all registered search providers"
        );
        let mut is_failed = false;
        for provider in PROVIDERS {
            match server
                .interface::<_, JetbrainsProductSearchProvider>(provider.objpath())
                .await
            {
                Err(error) => {
                    event!(
                        Level::DEBUG,
                        "Skipping {} ({}): {error}",
                        provider.label,
                        provider.desktop_id
                    );
                }
                Ok(search_provider_interface) => {
                    if let Err(error) = search_provider_interface
                        .get_mut()
                        .await
                        .reload_recent_projects()
                    {
                        is_failed = true;
                        let iface = search_provider_interface.get().await;
                        let app_id = iface.app().id();
                        event!(Level::ERROR, %app_id, "Failed to reload recent projects of {}: {}", app_id, error);
                    }
                }
            }
        }
        if is_failed {
            Err(zbus::fdo::Error::Failed(
                "Failed to reload recent projects of some providers".to_string(),
            ))
        } else {
            Ok(())
        }
    }
}
