// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Reload all recent projects across all exposed provider interfaces.

use tracing::{event, instrument, Level};
use zbus::{interface, ObjectServer};

use crate::searchprovider::JetbrainsProductSearchProvider;
use crate::{providers::PROVIDERS, ProviderDefinition};

/// Reload recent projects of a single `provider` on the given object `server`.
async fn reload_provider_on_object_server(
    server: &ObjectServer,
    provider: &ProviderDefinition<'_>,
) -> anyhow::Result<()> {
    let app_id = provider.desktop_id;
    event!(
        Level::DEBUG,
        %app_id,
        "Reloading recent projects of search provider registered at {}",
        provider.objpath()
    );
    let maybe_interface = server
        .interface::<_, JetbrainsProductSearchProvider>(provider.objpath())
        .await
        .map_err(|error| {
            event!(
                Level::DEBUG,
                %app_id,
                "Skipping {}: {error}",
                provider.label,
            );
        })
        .ok();

    match maybe_interface {
        Some(interface) => interface.get_mut().await.reload_recent_projects(),
        None => Ok(()),
    }
}

/// Reload all providers registered on the given object `server`.
pub async fn reload_all_on_object_server(server: &ObjectServer) -> zbus::fdo::Result<()> {
    event!(
        Level::DEBUG,
        "Reloading recent projects of all registered search providers"
    );
    let mut is_failed = false;
    for provider in PROVIDERS {
        if let Err(error) = reload_provider_on_object_server(server, provider).await {
            is_failed = true;
            let app_id = provider.desktop_id;
            event!(Level::ERROR, %app_id, "Failed to reload recent projects of {}: {}", app_id, error);
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
        reload_all_on_object_server(server).await
    }
}
