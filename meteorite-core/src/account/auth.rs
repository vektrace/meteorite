/*
    meteorite  Fast, Secure & Easy-to-use Matrix client in Rust
    Copyright (C) 2026  Vektrace

    This program is free software: you can redistribute it and/or modify
    it under the terms of the GNU Affero General Public License as
    published by the Free Software Foundation, either version 3 of the
    License, or (at your option) any later version.

    This program is distributed in the hope that it will be useful,
    but WITHOUT ANY WARRANTY; without even the implied warranty of
    MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
    GNU Affero General Public License for more details.

    You should have received a copy of the GNU Affero General Public License
    along with this program.  If not, see <https://www.gnu.org/licenses/>.
*/

use super::SecureAccountData;
use crate::{
    INITIAL_DEVICE_NAME,
    storage::{self, StorageApi},
};
use matrix_sdk::{
    Client, RefreshTokenError, SessionChange,
    ruma::{
        OwnedDeviceId,
        api::{FeatureFlag, client::session::get_login_types::v3::LoginType, error::ErrorKind},
    },
};
use tokio::sync::{mpsc, oneshot};

// The files in this folder contain all functions for authenticating a user. That includes loading necessary
// config files, getting passwords from keyring and using matrix sdks functions to send requests to
// the users homeserver.

// SESSION STORING
// store sessions in encrypted files (using age crate) and store identity to tht in keyring
// user_ids are stored in unencrypted with information if its active or not

pub enum LoginChoice {
    /// Password login
    Password,
    /// OAuth 2.0 login
    Oauth {
        /// Wether the client should ignore all other methods and only advertise OAuth
        preferred: bool,
        /// Whether the client supports logging in with a QR Code
        supports_msc4108: bool,
    },
}

/// Retrieves all supported login types for a given homeserver.
///
/// Returns an empty list if no compatible login methods (like Password
/// or OAuth) are discovered. The UI must handle this fallback.
///
/// # Errors
/// An error will be returned if connecting to the homeserver or any requests fail.
pub async fn get_login_types(homeserver: String) -> anyhow::Result<Vec<LoginChoice>> {
    let client = Client::builder()
        .server_name_or_homeserver_url(homeserver)
        .build()
        .await?;

    let mut types = Vec::new();
    let login_types = client.matrix_auth().get_login_types().await?.flows;

    let oauth_supported = if let Err(err) = client.oauth().server_metadata().await {
        if err.is_not_supported() {
            false
        } else {
            return Err(err.into());
        }
    } else {
        true
    };

    let supports_msc4108 = client
        .unstable_features()
        .await?
        .contains(&FeatureFlag::Msc4108);

    if login_types
        .iter()
        .any(|t| matches!(t, LoginType::Password(_)))
    {
        types.push(LoginChoice::Password);
    }

    if oauth_supported {
        let preferred = login_types
            .iter()
            .find_map(|t| match t {
                LoginType::Sso(sso) => Some(sso.oauth_aware_preferred),
                _ => None,
            })
            .unwrap_or(false);

        types.push(LoginChoice::Oauth {
            preferred,
            supports_msc4108,
        });
    }

    // ui has to handle empty types
    Ok(types)
}

/// Handles refreshing access tokens.
///
/// When an access token is refreshed, the new data (token, refresh token, expiration) will be
/// updated in the respective file and a new client will be sent.
///
/// Errors that should be displayed by the UI will be sent over the provided `Sender`.
///
/// This function will return when the current user session should be discarded. In that case the
/// user should be:
/// 1. prompted to switch account
/// 2. brought back to the login screen.
///
/// Returns the `OwnedDeviceId` of the discarded session if soft-logout could not be recovered (no
/// usable refresh token), so the user can be logged back in with the same device id instead of
/// creating a new one.
pub async fn handle_refresh_tokens(
    client: Client,
    tx: mpsc::Sender<anyhow::Error>,
) -> Option<OwnedDeviceId> {
    let client = client.clone();
    let mut session_change_stream = client.subscribe_to_session_changes();

    let (done_tx, done_rx) = oneshot::channel();

    tokio::spawn(async move {
        while let Ok(change) = session_change_stream.recv().await {
            if let SessionChange::UnknownToken(data) = change {
                if data.soft_logout {
                    let res = client.refresh_access_token().await;
                    match res {
                        Ok(()) => {
                            let tokens = client
                                .session_tokens()
                                .expect("Client should be authenticated");

                            if let Ok(id) = storage::Core::user_id_to_id(
                                client.user_id().expect("Client should be authenticated"),
                            ) {
                                match storage::Core::update_tokens(
                                    &id,
                                    Some(tokens.access_token),
                                    Some(tokens.refresh_token),
                                ) {
                                    Ok(()) => {}
                                    Err(e) => {
                                        let _ = tx.send(anyhow::anyhow!(e)).await;
                                    }
                                }
                            }
                        }
                        Err(err) => {
                            // check for another soft logout/no available refresh token (exchange device id for new access token)
                            match err {
                                RefreshTokenError::RefreshTokenRequired => {}
                                RefreshTokenError::MatrixAuth(http_error)
                                    if matches!(
                                        http_error.client_api_error_kind(),
                                        Some(ErrorKind::UnknownToken(_))
                                    ) => {}
                                err => {
                                    let _ = tx.send(err.into()).await;
                                    return;
                                }
                            }

                            if let Ok(id) = storage::Core::user_id_to_id(
                                client.user_id().expect("Client should be authenticated"),
                            ) {
                                storage::Core::remove(&id).ok();
                            }

                            let _ = done_tx.send(Some(
                                client
                                    .device_id()
                                    .expect("Client should be authenticated")
                                    .into(),
                            ));
                            return;
                        }
                    }
                } else {
                    // session destroyed, dont reuse
                    if let Ok(id) = storage::Core::user_id_to_id(
                        client.user_id().expect("Client should be authenticated"),
                    ) {
                        storage::Core::remove(&id).ok();
                    }

                    let _ = done_tx.send(None);
                    return;
                }
            }
        }
    });

    done_rx.await.ok().flatten()
}

/// Tries to log the user into the currently active account.
///
/// Also reads the files necessary to login the user (users.toml, encrypted files, keyring entry).
/// On success, an authenticated Client is returned.
///
/// Can return `None` instead of a client when there is no active account
pub async fn login() -> anyhow::Result<Option<Client>> {
    storage::Core::cleanup().await;

    let accounts = storage::Core::account_list()?;
    let Some(active) = accounts.active() else {
        return Ok(None);
    };

    let (storage, account) = storage::Core::load(&active.id)?;

    // construct the client
    let client = storage
        .setup_matrix_store()
        .server_name_or_homeserver_url(account.data.user_id.server_name())
        .build()
        .await?;

    // restore session from the unified account struct
    client.restore_session(account).await?;

    Ok(Some(client))
}

/// Tries to log a user in with the provided homeserver, username and password.
///
/// If a device id is provided, it will be used instead of creating a new one.
///
/// Also saves the new data (users.toml, encrypted file, keyring entry)
/// On success, an authenticated Client is returned.
pub async fn login_username(
    homeserver: String,
    username: String,
    password: String,
    device_id: Option<OwnedDeviceId>,
) -> anyhow::Result<Client> {
    let storage = storage::Core::new();

    // construct client
    let client = storage
        .setup_matrix_store()
        .server_name_or_homeserver_url(homeserver)
        .build()
        .await?;

    let mut login_builder = client
        .matrix_auth()
        .login_username(&username, &password)
        .initial_device_display_name(
            INITIAL_DEVICE_NAME
                .get()
                .expect("initial device name should be initialized"),
        )
        .request_refresh_token();

    if let Some(device_id) = device_id {
        login_builder = login_builder.device_id(device_id.as_str());
    }

    // start login
    let response = login_builder.await?;

    // construct new secure account data from response
    let secure_data = SecureAccountData::new(
        response.access_token,
        response.refresh_token,
        response.device_id,
        None,
    );

    storage.save(&response.user_id, &secure_data)?;

    Ok(client)
}

pub mod oauth {
    use matrix_sdk::{
        authentication::oauth::{
            CsrfToken, OAuthAuthorizationData,
            registration::{ApplicationType, ClientMetadata, Localized, OAuthGrantType},
        },
        reqwest::Url,
        ruma::serde::Raw,
        utils::local_server::LocalServerBuilder,
    };
    use std::net::{Ipv4Addr, Ipv6Addr};

    use super::*;

    struct OAuthLoginGuard {
        client: Option<Client>,
        state: Option<CsrfToken>,
    }

    impl OAuthLoginGuard {
        fn new(client: Client) -> Self {
            Self {
                client: Some(client),
                state: None,
            }
        }

        fn arm(&mut self, data: &OAuthAuthorizationData) {
            self.state = Some(data.state.clone());
        }
    }

    impl Drop for OAuthLoginGuard {
        fn drop(&mut self) {
            if let (Some(client), Some(state)) = (self.client.take(), self.state.take())
                && let Ok(handle) = tokio::runtime::Handle::try_current()
            {
                handle.spawn(async move { client.oauth().abort_login(&state).await });
            }
        }
    }

    fn metadata() -> ClientMetadata {
        let ipv4_localhost_uri = Url::parse(&format!("http://{}/", Ipv4Addr::LOCALHOST))
            .expect("Couldn't parse IPv4 redirect URI");
        let ipv6_localhost_uri = Url::parse(&format!("http://[{}]/", Ipv6Addr::LOCALHOST))
            .expect("Couldn't parse IPv6 redirect URI");
        let client_uri = Localized::new(
            // maybe one day we will get meteorite its own website...
            Url::parse("https://github.com/vektrace/meteorite").expect("Couldn't parse client URI"),
            None,
        );

        ClientMetadata {
            client_name: Some(Localized::new(
                INITIAL_DEVICE_NAME
                    .get()
                    .expect("initial device name should be initialized")
                    .clone(),
                [],
            )),
            // TODO: possibly add policy and tos uri?
            policy_uri: None,
            tos_uri: None,
            ..ClientMetadata::new(
                ApplicationType::Native,
                vec![
                    OAuthGrantType::AuthorizationCode {
                        redirect_uris: vec![ipv4_localhost_uri, ipv6_localhost_uri],
                    },
                    OAuthGrantType::DeviceCode,
                ],
                client_uri,
            )
        }
    }

    /// Tries to log a user in via the OAuth Authorization Code flow.
    ///
    /// Also saves the new data (users.toml, encrypted file, keyring entry)
    /// On success, an authenticated Client is returned.
    pub async fn login(
        homeserver: String,
        device_id: Option<OwnedDeviceId>,
        tx: mpsc::UnboundedSender<String>,
    ) -> anyhow::Result<Client> {
        let storage = storage::Core::new();

        // construct the client
        let client = storage
            .setup_matrix_store()
            .server_name_or_homeserver_url(homeserver)
            .build()
            .await?;

        let metadata = Raw::new(&metadata())?;

        let (redirect_uri, server_handle) = LocalServerBuilder::new().spawn().await?;

        let mut guard = OAuthLoginGuard::new(client.clone());

        let data = client
            .oauth()
            .login(redirect_uri, device_id, Some(metadata.into()), None)
            .build()
            .await?;
        guard.arm(&data);

        let query_string = {
            if webbrowser::open(data.url.as_str()).is_ok() {
                tx.send("Go to the opened website to authenticate".to_string())
                    .ok();
            } else {
                tx.send(format!("Navigate to {} in a browser of choice", data.url))
                    .ok();
            }

            server_handle.await
        };

        let Some(query_string) = query_string else {
            client.oauth().abort_login(&data.state).await;
            anyhow::bail!(
                "Error: failed to login: missing query string on the redirect URL\nPlease try again"
            );
        };

        if let Err(err) = client.oauth().finish_login(query_string.into()).await {
            anyhow::bail!("Error: failed to login: {err}");
        }

        let session = client
            .oauth()
            .full_session()
            .expect("Client should be logged in");

        // construct new secure account data from response
        let secure_data = SecureAccountData::new(
            session.user.tokens.access_token,
            session.user.tokens.refresh_token,
            session.user.meta.device_id,
            Some(session.client_id),
        );

        storage.save(&session.user.meta.user_id, &secure_data)?;

        Ok(client)
    }

    pub mod qr {
        use matrix_sdk::authentication::oauth::qrcode::{
            GeneratedQrProgress, LoginProgress, SenderError,
        };
        use matrix_sdk::stream::StreamExt;
        use qrcode_generator::{Renderer, qr::Encoder};

        use super::*;

        pub enum GeneratedQrState {
            /// Waiting for the QR code to be scanned with QR code data attached
            WaitingForScan(String),
            /// Waiting for check code to be transmitted
            WaitingForCheckCode,
            /// Waiting for access token, code attached which other device might ask for
            WaitingForToken(String),
        }

        /// Tries to log a user in by letting an authenticated device scan a QR code.
        ///
        /// Note that a custom device ID is not supported, so an existing one cannot be reused.
        ///
        /// The transmitter is used to transmit general instructions for the UI to display.
        /// The receiver is used to get the check code when needed.
        ///
        /// Also saves the new data (users.toml, encrypted file, keyring entry)
        /// On success, an authenticated Client is returned.
        pub async fn display(
            homeserver: String,
            tx: mpsc::UnboundedSender<GeneratedQrState>,
            mut rx: mpsc::UnboundedReceiver<u8>,
        ) -> anyhow::Result<Client> {
            let storage = storage::Core::new();

            // construct the client
            let client = storage
                .setup_matrix_store()
                .server_name_or_homeserver_url(homeserver)
                .build()
                .await?;

            let metadata = Raw::new(&metadata())?.into();
            let oauth = client.oauth();
            let login_client = oauth.login_with_qr_code(Some(&metadata)).generate();
            let mut subscriber = login_client.subscribe_to_progress();

            let task = tokio::spawn(async move {
                while let Some(state) = subscriber.next().await {
                    match state {
                        LoginProgress::Starting | LoginProgress::SyncingSecrets => (),
                        LoginProgress::EstablishingSecureChannel(GeneratedQrProgress::QrReady(
                            data,
                        )) => {
                            let symbol = Encoder::new(qrcode_generator::qr::ErrorCorrection::Low)
                                .encode_bytes(data.to_bytes())
                                .unwrap();

                            let data = Renderer::new(&symbol, 256)
                                .quiet_zone(0)
                                .svg_xml_declaration(false)
                                .to_svg_string(None::<String>)
                                .unwrap();

                            tx.send(GeneratedQrState::WaitingForScan(data)).ok();
                        }
                        LoginProgress::EstablishingSecureChannel(
                            GeneratedQrProgress::QrScanned(sender),
                        ) => {
                            tx.send(GeneratedQrState::WaitingForCheckCode).ok();
                            let Some(check_code) = rx.recv().await else {
                                // Login cancelled or something else happened
                                break;
                            };

                            match sender.send(check_code).await {
                                Ok(())
                                | Err(SenderError::AlreadySent | SenderError::CannotSend) => {}
                            }
                        }
                        LoginProgress::WaitingForToken { user_code } => {
                            tx.send(GeneratedQrState::WaitingForToken(user_code)).ok();
                        }
                        LoginProgress::Done => break,
                    }
                }
            });

            let result = login_client.await;
            task.abort();

            result?;

            let session = client
                .oauth()
                .full_session()
                .expect("Client should be logged in");

            // construct new secure account data from response
            let secure_data = SecureAccountData::new(
                session.user.tokens.access_token,
                session.user.tokens.refresh_token,
                session.user.meta.device_id,
                Some(session.client_id),
            );

            storage.save(&session.user.meta.user_id, &secure_data)?;

            Ok(client)
        }

        pub enum QrState {
            /// Waiting for QR Code to be scanned
            WaitingForScan,
            /// Input attached check code on other device
            InputCheckCode(u8),
            /// Waiting for access token, code attached which other device might ask for
            WaitingForToken(String),
        }

        /// Tries to log a user in by scanning a QR code.
        ///
        /// Note that a custom device ID is not supported, so an existing one cannot be reused.
        ///
        /// Also saves the new data (users.toml, encrypted file, keyring entry)
        /// On success, an authenticated Client is returned.
        #[cfg(any(target_os = "android", target_os = "ios"))]
        pub async fn scan(tx: mpsc::UnboundedSender<QrState>) -> anyhow::Result<Client> {
            use camera::{
                CameraSystem, CaptureProfile, ConversionRequest, DeviceSelector, RgbConverter,
            };
            use image::{DynamicImage, RgbImage};
            use matrix_sdk::authentication::oauth::qrcode::{
                Msc4108IntentData, QrCodeData, QrCodeIntentData, QrProgress,
            };

            let storage = storage::Core::new();

            let system = CameraSystem::new();
            let mut capture = system
                .capture(DeviceSelector::Default)
                .profile(CaptureProfile::Preview)
                .start()
                .await?;

            tx.send(QrState::WaitingForScan).ok();

            let decoded = loop {
                let Ok(frame) = capture.next_frame().await else {
                    // maybe more luck on the next frame?
                    continue;
                };

                let layout = frame.layout();
                let width = layout.width;
                let height = layout.height;

                // convert to rgb before converting to grayscale
                let request = ConversionRequest::for_layout(layout);
                let mut rgb_data = vec![0; request.output_len().unwrap()];

                let mut converter = RgbConverter::new();
                converter
                    .convert_into(&frame, request, &mut rgb_data)
                    .unwrap();

                let rgb_image = RgbImage::from_raw(width, height, rgb_data).unwrap();
                let img = DynamicImage::ImageRgb8(rgb_image).into_luma8();

                let mut decoder = quircs::Quirc::default();

                let mut codes = decoder.identify(img.width() as usize, img.height() as usize, &img);

                let decoded = codes.find_map(|c| {
                    let c = c.ok()?;

                    c.decode().ok()
                });

                if let Some(decoded) = decoded {
                    break decoded.payload;
                }

                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            };

            capture.close().await.unwrap();

            let qr_data = QrCodeData::from_bytes(&decoded)?;

            let QrCodeIntentData::Msc4108 {
                data: Msc4108IntentData::Reciprocate { server_name },
                ..
            } = &qr_data.intent_data()
            else {
                anyhow::bail!("QR code invalid, no homeserver received in the QR code.");
            };

            // construct the client
            let client = storage
                .setup_matrix_store()
                .server_name_or_homeserver_url(server_name)
                .build()
                .await?;

            let metadata = Raw::new(&metadata())?.into();
            let oauth = client.oauth();

            let login_client = oauth.login_with_qr_code(Some(&metadata)).scan(&qr_data);
            let mut subscriber = login_client.subscribe_to_progress();

            let task = tokio::spawn(async move {
                while let Some(state) = subscriber.next().await {
                    match state {
                        LoginProgress::Starting | LoginProgress::SyncingSecrets => (),
                        LoginProgress::EstablishingSecureChannel(QrProgress { check_code }) => {
                            tx.send(QrState::InputCheckCode(check_code)).ok();
                        }
                        LoginProgress::WaitingForToken { user_code } => {
                            tx.send(QrState::WaitingForToken(user_code)).ok();
                        }
                        LoginProgress::Done => break,
                    }
                }
            });

            let result = login_client.await;
            task.abort();

            result?;

            let session = client
                .oauth()
                .full_session()
                .expect("Client should be logged in");

            // construct new secure account data from response
            let secure_data = SecureAccountData::new(
                session.user.tokens.access_token,
                session.user.tokens.refresh_token,
                session.user.meta.device_id,
                Some(session.client_id),
            );

            storage
                .save(&session.user.meta.user_id, &secure_data)
                .await?;

            Ok(client)
        }
    }
}
