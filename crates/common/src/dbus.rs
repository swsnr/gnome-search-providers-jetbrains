// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! DBus helpers for search providers.

use std::collections::HashMap;
use std::fmt::Debug;

use gio::{AppInfoExt, IconExt};
use thiserror::Error;
use zbus::export::zvariant;
use zbus::fdo::{DBusProxy, RequestNameFlags, RequestNameReply};
use zbus::{dbus_interface, Connection};

use log::{debug, error, info};

use crate::{find_matching_items, IdMap, IndexMap, ItemsSource, RecentItem, ScoreMatchable};

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
    let reply = DBusProxy::new(&connection)?
        .request_name(name.as_ref(), RequestNameFlags::DoNotQueue.into())?;
    if reply == RequestNameReply::PrimaryOwner {
        Ok(())
    } else {
        Err(AcquireNameError::RequestNameRejected(
            name.as_ref().to_string(),
            reply,
        ))
    }
}

/// A search provider for recent items.
pub struct RecentItemSearchProvider<S: ItemsSource<T>, T: RecentItem + ScoreMatchable> {
    app: gio::DesktopAppInfo,
    source: S,
    items: IdMap<T>,
}

impl<S: ItemsSource<T>, T: RecentItem + ScoreMatchable> RecentItemSearchProvider<S, T> {
    /// Create a new search provider for recent items of `app`.
    ///
    /// Uses the given `source` to load recent items.
    pub fn new(app: gio::DesktopAppInfo, source: S) -> Self {
        Self {
            app,
            source,
            items: IndexMap::new(),
        }
    }
}

/// The DBus interface of the search provider.
///
/// See <https://developer.gnome.org/SearchProvider/> for information.
#[dbus_interface(name = "org.gnome.Shell.SearchProvider2")]
impl<S: ItemsSource<T> + 'static, T: RecentItem + ScoreMatchable + Debug + 'static>
    RecentItemSearchProvider<S, T>
{
    /// Starts a search.
    ///
    /// This function is called when a new search is started. It gets an array of search terms as arguments,
    /// and should return an array of result IDs. gnome-shell will call GetResultMetas for (some) of these result
    /// IDs to get details about the result that can be be displayed in the result list.
    fn get_initial_result_set(&mut self, terms: Vec<String>) -> zbus::fdo::Result<Vec<String>> {
        debug!(
            "Searching for {:?} of {}",
            terms,
            self.app.get_id().unwrap()
        );
        self.items = self.source.find_recent_items().map_err(|error| {
            error!(
                "Failed to update recent items for {}: {:#}",
                self.app.get_id().unwrap(),
                error
            );
            zbus::fdo::Error::Failed(format!(
                "Failed to update recent items for {}: {:#}",
                self.app.get_id().unwrap(),
                error
            ))
        })?;

        let ids = find_matching_items(self.items.iter(), terms.as_slice())
            .into_iter()
            .map(String::to_owned)
            .collect();
        debug!("Found ids {:?} for {}", ids, self.app.get_id().unwrap());
        Ok(ids)
    }

    /// Refine an ongoing search.
    ///
    /// This function is called to refine the initial search results when the user types more characters in the search entry.
    /// It gets the previous search results and the current search terms as arguments, and should return an array of result IDs,
    /// just like GetInitialResulSet.
    fn get_subsearch_result_set(
        &self,
        previous_results: Vec<String>,
        terms: Vec<String>,
    ) -> Vec<String> {
        debug!(
            "Searching for {:?} in {:?} of {}",
            terms,
            previous_results,
            self.app.get_id().unwrap()
        );
        let candidates = previous_results
            .iter()
            .filter_map(|id| self.items.get(id).map(|p| (id, p)));

        let ids = find_matching_items(candidates, terms.as_slice())
            .into_iter()
            .map(String::to_owned)
            .collect();
        debug!("Found ids {:?} for {}", ids, self.app.get_id().unwrap());
        ids
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
    //  - "gicon": a textual representation of a GIcon (see g_icon_to_string()), or alternativly,
    //  - "icon-data": a tuple of type (iiibiiay) describing a pixbuf with width, height, rowstride, has-alpha, bits-per-sample, and image data
    //  - "description": an optional short description (1-2 lines)
    fn get_result_metas(&self, results: Vec<String>) -> Vec<HashMap<String, zvariant::Value>> {
        debug!("Getting meta info for {:?}", results);
        results
            .into_iter()
            .filter_map(|id| {
                self.items.get(&id).map(|item| {
                    debug!("Compiling meta infor for {}", id);
                    let icon = IconExt::to_string(&self.app.get_icon().unwrap()).unwrap();
                    debug!("Using icon {} for id {}", icon, id);

                    let mut meta: HashMap<String, zvariant::Value> = HashMap::new();
                    meta.insert("id".to_string(), id.into());
                    meta.insert("name".to_string(), item.name().to_string().into());
                    meta.insert("gicon".to_string(), icon.to_string().into());
                    meta.insert(
                        "description".to_string(),
                        item.description().to_string().into(),
                    );
                    meta
                })
            })
            .collect()
    }

    /// Activate an individual result.
    ///
    /// This function is called when the user clicks on an individual result to open it in the application.
    /// The arguments are the result ID, the current search terms and a timestamp.
    ///
    /// Launches the underlying app with the path to the selected item.
    fn activate_result(
        &self,
        id: String,
        terms: Vec<String>,
        timestamp: u32,
    ) -> zbus::fdo::Result<()> {
        debug!("Activating result {} for {:?} at {}", id, terms, timestamp);
        if let Some(item) = self.items.get(&id) {
            info!("Launching recent item {:?}", item);
            self.app
                .launch_uris::<gio::AppLaunchContext>(&[item.uri()], None)
                .map_err(|error| {
                    error!(
                        "Failed to launch app {} for URI {}: {}",
                        self.app.get_id().unwrap(),
                        item.uri(),
                        error
                    );
                    zbus::fdo::Error::SpawnFailed(format!(
                        "Failed to launch app {} for URI {}: {}",
                        self.app.get_id().unwrap(),
                        item.uri(),
                        error
                    ))
                })
        } else {
            error!("Item with ID {} not found", id);
            Err(zbus::fdo::Error::Failed(format!("Result {} not found", id)))
        }
    }

    /// Launch a search within the App.
    ///
    /// This function is called when the user clicks on the provider icon to display more search results in the application.
    /// The arguments are the current search terms and a timestamp.
    ///
    /// Currently it simply launches the app without any arguments.
    fn launch_search(&self, terms: Vec<String>, timestamp: u32) -> zbus::fdo::Result<()> {
        debug!("Launching search for {:?} at {}", terms, timestamp);
        info!("Launching app {} directly", self.app.get_id().unwrap());
        self.app
            .launch::<gio::AppLaunchContext>(&[], None)
            .map_err(|error| {
                error!(
                    "Failed to launch app {}: {:#}",
                    self.app.get_id().unwrap(),
                    error
                );
                zbus::fdo::Error::SpawnFailed(format!(
                    "Failed to launch app {}: {}",
                    self.app.get_id().unwrap(),
                    error
                ))
            })
    }
}
