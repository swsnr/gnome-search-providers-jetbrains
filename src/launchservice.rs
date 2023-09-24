// Copyright Sebastian Wiesner <sebastian@swsnr.de>
//
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! A service to launch apps on a given Glib main context.

use crate::systemd::*;
use gio::prelude::*;
use glib::VariantDict;
use glib::{ControlFlow, SourceId};
use std::convert::TryFrom;
use std::fmt::{Display, Formatter};
use std::sync::Arc;
use tracing::field;
use tracing::{debug, error, info, span, trace, warn};
use tracing::{instrument, Level, Span};
use tracing_futures::Instrument;
use zbus::zvariant::OwnedObjectPath;

/// Settings for systemd scopes created by a search provider for launched apps.
#[derive(Debug, Clone)]
pub struct SystemdScopeSettings {
    /// The prefix for scope names.
    pub prefix: String,
    /// A string which identifies the app which started the scope.
    pub started_by: String,
    /// Optional documentation URLs for the scope.
    pub documentation: Vec<String>,
}

#[instrument(skip(connection))]
async fn move_launched_process_to_scope(
    connection: &zbus::Connection,
    id: &str,
    description: &str,
    scope_settings: &SystemdScopeSettings,
    pid: u32,
) -> Result<(String, OwnedObjectPath), zbus::Error> {
    // Gnome also strips the .desktop suffix from IDs, see
    // https://gitlab.gnome.org/GNOME/gnome-desktop/-/blob/106a729c3f98b8ee56823a0a49fa8504f78dd355/libgnome-desktop/gnome-systemd.c#L227
    let properties = ScopeProperties {
        prefix: &scope_settings.prefix,
        name: id.trim_end_matches(".desktop"),
        description: Some(description),
        documentation: scope_settings
            .documentation
            .iter()
            .map(|v| v.as_str())
            .collect(),
    };
    trace!("Connect to Systemd1 Manager object");
    let systemd = Systemd1ManagerProxy::new(connection).await?;
    debug!(
        "Starting scope for process {} of app {} with properties: {:?}",
        pid, id, &properties
    );
    start_app_scope(&systemd, properties, pid).await
}

/// The desktop ID of an app.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct AppId(String);

impl Display for AppId {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        self.0.fmt(f)
    }
}

impl TryFrom<&AppId> for gio::DesktopAppInfo {
    type Error = glib::Error;

    fn try_from(value: &AppId) -> Result<Self, Self::Error> {
        gio::DesktopAppInfo::new(&value.0).ok_or_else(|| {
            glib::Error::new(
                glib::FileError::Noent,
                &format!("App {} not found", value.0),
            )
        })
    }
}

impl From<String> for AppId {
    fn from(v: String) -> Self {
        Self(v)
    }
}

impl From<&str> for AppId {
    fn from(v: &str) -> Self {
        v.to_string().into()
    }
}

impl From<&gio::DesktopAppInfo> for AppId {
    fn from(app: &gio::DesktopAppInfo) -> Self {
        AppId(app.id().unwrap().to_string())
    }
}

/// An app that can be launched.
#[derive(Debug)]
pub struct App {
    /// The ID of this app
    id: AppId,
    /// The icon to use for this app
    icon: String,
}

impl App {
    /// The ID of this app.
    pub fn id(&self) -> &AppId {
        &self.id
    }

    /// The icon of this app.
    pub fn icon(&self) -> &str {
        &self.icon
    }
}

impl From<gio::DesktopAppInfo> for App {
    fn from(app: gio::DesktopAppInfo) -> Self {
        Self {
            id: (&app).into(),
            icon: IconExt::to_string(&app.icon().unwrap())
                .unwrap()
                .to_string(),
        }
    }
}

/// Request to launch an app, with an URI to launch with.
#[derive(Debug)]
struct AppLaunchRequest {
    /// The ID of the app to launch.
    app: AppId,
    /// The URI to pass to the app on launch.
    uri: Option<String>,
    /// A one-shot channel to send the result of launching the app to.
    response: futures_channel::oneshot::Sender<Result<(), glib::Error>>,
    /// The span in which to trace launching.
    span: Span,
}

async fn launch_app(
    context: &gio::AppLaunchContext,
    app_id: &AppId,
    uri: Option<&str>,
) -> Result<(), glib::Error> {
    let app = gio::DesktopAppInfo::try_from(app_id)?;
    debug!("Launching App {:?} with uri {:?}", app, &uri);
    match uri {
        None => app.launch_uris_future(&[], Some(context)),
        Some(uri) => app.launch_uris_future(&[uri], Some(context)),
    }
    .await
}

#[instrument(skip(connection))]
fn handle_launched(
    connection: zbus::Connection,
    scope_settings: Arc<SystemdScopeSettings>,
    app: &gio::AppInfo,
    platform_data: &glib::Variant,
) {
    match platform_data.get::<VariantDict>() {
        None => error!("platform_data not a dictionary, but {:?}", platform_data),
        // The type of the pid property doesn't seem to be documented anywhere, but variant type
        // errors indicate that the type is "i", i.e.gint32.
        //
        // See https://docs.gtk.org/glib/gvariant-format-strings.html#numeric-types
        Some(data) => match data.lookup::<i32>("pid") {
            Err(type_error) => {
                error!(
                    "platform_data.pid had type {:?}, but expected {:?}",
                    type_error.actual, type_error.expected
                );
            }
            Ok(None) => {
                warn!("pid missing in platform_data {:?}", platform_data);
            }
            Ok(Some(pid)) => {
                info!("App {} launched with PID {}", app.id().unwrap(), pid);
                let id = app.id().unwrap().to_string();
                let description = app.description().map_or_else(
                    || format!("app started by {}", scope_settings.started_by),
                    |value| format!("{} started by {}", value, scope_settings.started_by),
                );
                glib::MainContext::ref_thread_default().spawn(async move {
                    let result = move_launched_process_to_scope(
                        &connection,
                        &id,
                        &description,
                        &scope_settings,
                        // Systemd uses u32 to represent PIDs, so let's cast.
                        pid as u32,
                    )
                        .await;
                    match result {
                        Err(err) => {
                            error!("Failed to move running process {} of app {} into new systemd scope: {}", pid, id, err);
                        },
                        Ok((name, path)) => {
                            info!("Moved running process {} of app {} into new systemd scope {} at {}",pid, id, &name, path.into_inner());
                        },
                    };
                }.in_current_span());
            }
        },
    }
}

fn handle_launch(launch_context: gio::AppLaunchContext, request: AppLaunchRequest) {
    glib::MainContext::ref_thread_default().spawn_local(async move {
        // We don't care if the receiver already dropped their side of the channel
        let _ = request.response.send(
            launch_app(&launch_context, &request.app, request.uri.as_deref())
                .instrument(
                    span!(parent: request.span.clone(), Level::INFO, "handle_launch", request.app = field::debug(&request.app), request.uri = field::debug(&request.uri)),
                )
                .await,
        );
    });
}

/// A service which launches apps on a glib main context.
///
/// A launch service receives requests to launch apps from launch service clients.
///
/// For each request it then starts an instance of the app with the given URI if any,
/// and moves the new process into a dedicated systemd scope to isolate it.
#[derive(Debug)]
pub struct AppLaunchService {
    recv: glib::Receiver<AppLaunchRequest>,
    send: glib::Sender<AppLaunchRequest>,
}

impl Default for AppLaunchService {
    fn default() -> Self {
        let (send, recv) = glib::MainContext::channel(glib::Priority::default());
        AppLaunchService { send, recv }
    }
}

impl AppLaunchService {
    /// Create a new launch service.
    pub fn new() -> Self {
        Self::default()
    }

    /// Start this app launch service.
    ///
    /// `main_context` denotes the context on which to receive launch requests.
    /// `connection` is the DBus connection to use for talking to systemd in order to
    /// create new scopes for started instances.  `scope_settings` denotes the how
    /// these scopes should be set up.
    ///
    /// Consumes `self` in order to avoid starting a service twice.  Returns the client side
    /// of this service and the glib source ID which refers to the running service.
    ///
    /// To attach further clients simply clone the returned client.  To stop the service use
    /// [`glib::remove_source`] on the returned source ID.
    #[must_use]
    pub fn start(
        self,
        connection: zbus::Connection,
        scope_settings: SystemdScopeSettings,
    ) -> (AppLaunchClient, SourceId) {
        let scope_settings_arc = Arc::new(scope_settings);
        let launch_context = gio::AppLaunchContext::new();
        launch_context.connect_launched(move |_, app, platform_data| {
            trace!(
                "App {} launched with platform_data: {:?}",
                app.id().unwrap(),
                platform_data
            );
            handle_launched(
                connection.clone(),
                scope_settings_arc.clone(),
                app,
                platform_data,
            )
        });

        let client = self.client();
        let source = self.recv.attach(None, move |request: AppLaunchRequest| {
            handle_launch(launch_context.clone(), request);
            ControlFlow::Continue
        });
        (client, source)
    }

    /// Create a new client for this launch service.
    pub fn client(&self) -> AppLaunchClient {
        AppLaunchClient {
            send: self.send.clone(),
        }
    }
}

/// A client for a launch service.
///
/// Clients can be cloned cheaply to create a new separate client.
#[derive(Debug, Clone)]
pub struct AppLaunchClient {
    send: glib::Sender<AppLaunchRequest>,
}

impl AppLaunchClient {
    #[instrument(skip(self))]
    async fn launch(&self, app: AppId, uri: Option<String>) -> Result<(), glib::Error> {
        let (response_tx, response_rx) = futures_channel::oneshot::channel();
        let request = AppLaunchRequest {
            app,
            uri,
            span: Span::current(),
            response: response_tx,
        };
        trace!("Sending launch request {:?}", &request);
        self.send.send(request).map_err(|err| {
            glib::Error::new(
                glib::FileError::Failed,
                &format!("Failed to launch app: {err}"),
            )
        })?;
        // If the sender was dropped
        response_rx.await.unwrap_or_else(|_| {
            Err(glib::Error::new(
                glib::FileError::Failed,
                "App launch service did not reply",
            ))
        })
    }

    /// Launch an app without arguments
    pub async fn launch_app(&self, app: AppId) -> Result<(), glib::Error> {
        self.launch(app, None).await
    }

    /// Launch the app with an URI.
    pub async fn launch_uri(&self, app: AppId, uri: String) -> Result<(), glib::Error> {
        self.launch(app, Some(uri)).await
    }
}
