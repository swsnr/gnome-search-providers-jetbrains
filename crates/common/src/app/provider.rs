// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Search providers for apps.

use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

use futures_channel::{mpsc, oneshot};
use futures_util::SinkExt;
use indexmap::IndexMap;
use tracing::{event, instrument, Level, Span};
use zbus::dbus_interface;
use zbus::zvariant;

use crate::app::*;
use crate::matching::*;

/// Requests by the search provider.
#[derive(Debug)]
pub enum AppItemSearchRequest {
    /// Invalidate the list of items for the app.
    ///
    /// Pass a span for tracing.
    Invalidate(Span),
    /// Get app items.
    ///
    /// The first element is a span for tracing, the second the return channel for the items.
    GetItems(
        Span,
        oneshot::Sender<zbus::fdo::Result<Arc<IndexMap<String, AppLaunchItem>>>>,
    ),
}

impl AppItemSearchRequest {
    /// Get name of this request for debugging.
    pub fn name(&self) -> &'static str {
        use AppItemSearchRequest::*;
        match self {
            Invalidate(_) => "Invalidate",
            GetItems(_, _) => "GetItems",
        }
    }

    /// Get the span for tracing this request.
    pub fn span(&self) -> &tracing::Span {
        use AppItemSearchRequest::*;
        match self {
            Invalidate(span) => span,
            GetItems(span, _) => span,
        }
    }
}

/// A search provider for recent items.
#[derive(Debug)]
pub struct AppItemSearchProvider {
    launcher: AppLaunchClient,
    app: App,
    sender: mpsc::Sender<AppItemSearchRequest>,
}

impl AppItemSearchProvider {
    /// Create a new search provider for recent items of `app`.
    ///
    /// `launcher` is used to launch applications with Gio on the main thread, and `buffer`
    ///
    pub fn new(
        app: App,
        launcher: AppLaunchClient,
        sender: mpsc::Sender<AppItemSearchRequest>,
    ) -> Self {
        Self {
            launcher,
            app,
            sender,
        }
    }

    /// The app managed by this search provider.
    pub fn app(&self) -> &App {
        &self.app
    }

    /// Get items from the provider.
    async fn get_items(&mut self) -> zbus::fdo::Result<Arc<IndexMap<String, AppLaunchItem>>> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(AppItemSearchRequest::GetItems(Span::current(), tx))
            .await
            .map_err(|error| {
                event!(
                Level::ERROR,
                %error,
                "Failed to send request for search items: {}", error
                );
                zbus::fdo::Error::Failed("Failed to send request for search items".to_string())
            })?;
        let result = rx.await.map_err(|_| {
            event!(
                Level::ERROR,
                "App provider dropped while search provider was requesting items"
            );
            zbus::fdo::Error::Failed(
                "Failed to request app items, app provider dropped".to_string(),
            )
        })?;
        result.map_err(|error| {
            event!(Level::ERROR, %error, "Received error from app provider: {:#}", error);
            zbus::fdo::Error::Failed(format!("Failed to get app items: {error}"))
        })
    }
}

/// The DBus interface of the search provider.
///
/// See <https://developer.gnome.org/SearchProvider/> for information.
#[dbus_interface(name = "org.gnome.Shell.SearchProvider2")]
impl AppItemSearchProvider {
    /// Starts a search.
    ///
    /// This function is called when a new search is started. It gets an array of search terms as arguments,
    /// and should return an array of result IDs. gnome-shell will call GetResultMetas for (some) of these result
    /// IDs to get details about the result that can be be displayed in the result list.
    #[instrument(skip(self), fields(app_id = %self.app.id()))]
    async fn get_initial_result_set(&mut self, terms: Vec<&str>) -> zbus::fdo::Result<Vec<String>> {
        event!(Level::DEBUG, "Searching for {:?}", terms);
        event!(Level::DEBUG, "Refreshing items for new search");
        self.sender
            .feed(AppItemSearchRequest::Invalidate(Span::current()))
            .await
            .map_err(|error| {
                event!(
                Level::ERROR,
                %error,
                "Failed to send request to invalidate search items: {}", error
                );
                zbus::fdo::Error::Failed("Failed to invalidate search items".to_string())
            })?;
        let items = self.get_items().await?;
        let ids = find_matching_items(items.iter(), terms.as_slice())
            .into_iter()
            .map(String::to_owned)
            .collect();
        event!(Level::DEBUG, "Found ids {:?}", ids);
        Ok(ids)
    }

    /// Refine an ongoing search.
    ///
    /// This function is called to refine the initial search results when the user types more characters in the search entry.
    /// It gets the previous search results and the current search terms as arguments, and should return an array of result IDs,
    /// just like GetInitialResultSet.
    #[instrument(skip(self), fields(app_id = %self.app.id()))]
    async fn get_subsearch_result_set(
        &mut self,
        previous_results: Vec<&str>,
        terms: Vec<&str>,
    ) -> zbus::fdo::Result<Vec<String>> {
        event!(
            Level::DEBUG,
            "Searching for {:?} in {:?}",
            terms,
            previous_results
        );
        let items = self.get_items().await?;
        let candidates = previous_results
            .iter()
            .filter_map(|&id| items.get(id).map(|p| (id, p)));

        let ids = find_matching_items(candidates, terms.as_slice())
            .into_iter()
            .map(|s| s.to_owned())
            .collect();
        event!(Level::DEBUG, "Found ids {:?}", ids);
        Ok(ids)
    }

    /// Get metadata for results.
    ///
    /// This function is called to obtain detailed information for results.
    /// It gets an array of result IDs as arguments, and should return a matching array of dictionaries
    /// (ie one a{sv} for each passed-in result ID).
    ///
    /// The following pieces of information should be provided for each result:
    //
    //  - "id": the result ID
    //  - "name": the display name for the result
    //  - "icon": a serialized GIcon (see g_icon_serialize()), or alternatively,
    //  - "gicon": a textual representation of a GIcon (see g_icon_to_string()), or alternatively,
    //  - "icon-data": a tuple of type (iiibiiay) describing a pixbuf with width, height, rowstride, has-alpha, bits-per-sample, and image data
    //  - "description": an optional short description (1-2 lines)
    #[instrument(skip(self), fields(app_id = %self.app.id()))]
    async fn get_result_metas(
        &mut self,
        results: Vec<String>,
    ) -> zbus::fdo::Result<Vec<HashMap<String, zvariant::Value<'_>>>> {
        event!(Level::DEBUG, "Getting meta info for {:?}", results);
        let items = self.get_items().await?;
        let mut metas = Vec::with_capacity(results.len());
        for item_id in results {
            if let Some(item) = items.get(&item_id) {
                event!(Level::DEBUG, %item_id, "Compiling meta info for {}", item_id);
                let mut meta: HashMap<String, zvariant::Value> = HashMap::new();
                meta.insert("id".to_string(), item_id.clone().into());
                meta.insert("name".to_string(), item.name.clone().into());
                event!(Level::DEBUG, %item_id, "Using icon {}", self.app.icon());
                meta.insert("gicon".to_string(), self.app.icon().to_string().into());
                meta.insert("description".to_string(), item.uri.clone().into());
                metas.push(meta);
            }
        }

        event!(Level::DEBUG, "Return meta info {:?}", &metas);
        Ok(metas)
    }

    /// Activate an individual result.
    ///
    /// This function is called when the user clicks on an individual result to open it in the application.
    /// The arguments are the result ID, the current search terms and a timestamp.
    ///
    /// Launches the underlying app with the path to the selected item.
    #[instrument(skip(self), fields(app_id = %self.app.id()))]
    async fn activate_result(
        &mut self,
        item_id: &str,
        terms: Vec<&str>,
        timestamp: u32,
    ) -> zbus::fdo::Result<()> {
        event!(
            Level::DEBUG,
            item_id,
            "Activating result {} for {:?} at {}",
            item_id,
            terms,
            timestamp
        );
        let items = self.get_items().await?;
        if let Some(item) = items.get(item_id) {
            event!(Level::INFO, item_id, "Launching recent item {:?}", item);
            self.launcher
                .launch_uri(self.app.id().clone(), item.uri.clone())
                .await
                .map_err(|error| {
                    event!(
                        Level::ERROR,
                        %error,
                        "Failed to launch app {} for {:?}: {:#}",
                        self.app.id(),
                        item.uri,
                        error
                    );
                    zbus::fdo::Error::Failed(format!(
                        "Failed to launch app {} for {}: {}",
                        self.app.id(),
                        item.uri,
                        error
                    ))
                })
        } else {
            event!(Level::ERROR, item_id, "Item not found");
            Err(zbus::fdo::Error::Failed(format!(
                "Result {item_id} not found"
            )))
        }
    }

    /// Launch a search within the App.
    ///
    /// This function is called when the user clicks on the provider icon to display more search results in the application.
    /// The arguments are the current search terms and a timestamp.
    ///
    /// Currently it simply launches the app without any arguments.
    #[instrument(skip(self), fields(app_id = %self.app.id()))]
    async fn launch_search(&self, _terms: Vec<String>, _timestamp: u32) -> zbus::fdo::Result<()> {
        event!(Level::DEBUG, "Launching app directly");
        self.launcher
            .launch_app(self.app.id().clone())
            .await
            .map_err(|error| {
                event!(Level::ERROR, %error, "Failed to launch app {}: {:#}", self.app.id(), error);
                zbus::fdo::Error::Failed(format!(
                    "Failed to launch app {}: {}",
                    self.app.id(),
                    error
                ))
            })
    }
}
