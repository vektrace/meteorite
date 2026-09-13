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

use crate::{ACCOUNT_PATH, APP_NAME, INITIAL_DEVICE_NAME, utils};
use age::secrecy::SecretString;
use keyring_core::Entry;
use matrix_sdk::{
    AuthSession, Client, RefreshTokenError, SessionChange, SessionMeta, SessionTokens,
    authentication::{
        matrix::MatrixSession,
        oauth::{ClientId, OAuthSession, UserSession},
    },
    ruma::{
        OwnedDeviceId, OwnedUserId,
        api::{
            client::session::get_login_types::v3::{IdentityProvider, LoginType},
            error::ErrorKind,
        },
    },
};
use rand::distr::{Alphanumeric, SampleString};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::{fs, path::Path, path::PathBuf};
use tokio::sync::mpsc;

// This file contains all functions for authenticating a user. That includes loading necessary
// config files, getting passwords from keyring and using matrix sdks functions to send requests to
// the users homeserver.

// SESSION STORING
// store sessions in encrypted files (using age crate) and store passphrase to tht in keyring
// user_ids are stored in unencrypted with information if its active or not

// this struct only exists for toml
#[derive(Default, Deserialize, Serialize, Clone)]
struct AccountList {
    accounts: Vec<AccountData>,
}

// used for restoring sessions
struct Account {
    data: AccountData,
    secure_data: SecureAccountData,
}

// stored in unencrypted file, lets the client decide which data to load
#[derive(Deserialize, Serialize, Clone)]
struct AccountData {
    // id used for files instead of user_id
    id: String,
    user_id: OwnedUserId,
    active: bool,
}

// stored in encrypted file, passphrase stored in keyring
// only loaded when required
#[derive(Deserialize, Serialize)]
struct SecureAccountData {
    access_token: String,
    refresh_token: Option<String>,
    device_id: OwnedDeviceId,
    // only present when logged in via oauth
    client_id: Option<ClientId>,
}

impl SecureAccountData {
    fn new(
        access_token: String,
        refresh_token: Option<String>,
        device_id: OwnedDeviceId,
        client_id: Option<ClientId>,
    ) -> Self {
        Self {
            access_token,
            refresh_token,
            device_id,
            client_id,
        }
    }
}

// guard to clean up failed account creations
struct AccountCreationGuard {
    backup_path: PathBuf,
    backup_created: bool,

    users_path: PathBuf,

    sqlite_path: PathBuf,
    secure_path: PathBuf,

    users_tmp_path: PathBuf,
    secure_tmp_path: PathBuf,
    backup_tmp_path: PathBuf,

    keyring_entry: Entry,
    keyring_created: bool,

    committed: bool,
}

impl AccountCreationGuard {
    fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for AccountCreationGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.secure_tmp_path);
        let _ = fs::remove_file(&self.users_tmp_path);
        let _ = fs::remove_file(&self.backup_tmp_path);

        if self.committed {
            return;
        }

        let _ = fs::remove_file(&self.secure_path);
        let _ = fs::remove_dir_all(&self.sqlite_path);

        if self.backup_created {
            let _ = fs::rename(&self.backup_path, &self.users_path);
        }

        if self.keyring_created {
            let _ = self.keyring_entry.delete_credential();
        }
    }
}

#[derive(Debug)]
enum AccountResource {
    Folder(String, PathBuf),
    File(String, PathBuf),
    KeyringEntry(String),
    User(String),
}

const USER: u8 = 1 << 0;
const FILE: u8 = 1 << 1;
const FOLDER: u8 = 1 << 2;
const KEYRING: u8 = 1 << 3;

const ALL: u8 = USER | FILE | FOLDER | KEYRING;

pub enum LoginChoice {
    /// Password login
    Password,
    /// SSO login
    Sso {
        /// The identity providers (e.g. Github)
        identity_providers: Vec<IdentityProvider>,
    },
    /// OAuth 2.0 login
    Oauth {
        /// Wether the client should ignore all other methods and only advertise OAuth
        preferred: bool,
    },
}

/// Retrieves all supported login types for a given homeserver.
///
/// Returns an empty list if no compatible login methods (like Password,
/// SSO, or OAuth) are discovered. The UI must handle this fallback.
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

    if login_types
        .iter()
        .any(|t| matches!(t, LoginType::Password(_)))
    {
        types.push(LoginChoice::Password);
    }

    if let Some(sso) = login_types.iter().find_map(|t| match t {
        LoginType::Sso(sso) => Some(sso),
        _ => None,
    }) {
        if oauth_supported {
            types.push(LoginChoice::Oauth {
                preferred: sso.oauth_aware_preferred,
            })
        }

        // also check for `LoginType::Token`, if it is unsupported, login_token will fail
        if login_types.iter().any(|t| matches!(t, LoginType::Token(_))) {
            types.push(LoginChoice::Sso {
                identity_providers: sso.identity_providers.clone(),
            });
        }
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
pub async fn handle_refresh_tokens(client: Client, tx: mpsc::Sender<anyhow::Error>) {
    let client = client.clone();
    let mut session_change_stream = client.subscribe_to_session_changes();

    tokio::spawn(async move {
        while let Ok(change) = session_change_stream.recv().await {
            if let SessionChange::UnknownToken(data) = change {
                if data.soft_logout {
                    let res = client.refresh_access_token().await;
                    match res {
                        Ok(_) => {
                            let tokens = client
                                .session_tokens()
                                .expect("Client should be authenticated");

                            match update_account_config(
                                client
                                    .user_id()
                                    .expect("Client should be authenticated")
                                    .as_str(),
                                Some(tokens.access_token),
                                Some(tokens.refresh_token),
                            ) {
                                Ok(()) => {}
                                Err(e) => {
                                    let _ = tx.send(e).await;
                                }
                            }
                        }
                        Err(err) => {
                            // check for another soft logout/no available refresh token (exchange device id for new access token)
                            // TODO: send back device id as well (to not create a new one)
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

                            remove_account_config(
                                client
                                    .user_id()
                                    .expect("Client should be authenticated")
                                    .as_str(),
                            );
                            return;
                        }
                    }
                } else {
                    // session destroyed, dont reuse
                    remove_account_config(
                        client
                            .user_id()
                            .expect("Client should be authenticated")
                            .as_str(),
                    );
                    return;
                }
            }
        }
    });
}

/// Tries to log the user into the currently active account.
///
/// Also reads the files necessary to login the user (users.toml, encrypted files, keyring entry).
/// On success, an authenticated Client is returned.
///
/// Can return `None` instead of a client when there is no active account
pub async fn login() -> anyhow::Result<Option<Client>> {
    // first remove possible leftovers
    remove_orphaned_accounts();

    let account_path = utils::unwrap_lock(&ACCOUNT_PATH);
    let users_path = account_path.join("users.toml");

    let accounts = load_account_list(&users_path)?;
    let Some(account_data) = active_account(&accounts.accounts) else {
        return Ok(None);
    };

    // define the paths once
    let secure_path = account_path.join(format!("{}.enc", account_data.id));
    let sqlite_path = account_path.join(&account_data.id);

    let encryption_passphrase = load_encryption_passphrase(&account_data.id)?;

    let secure_account_data = load_secure_account_data(&secure_path, &encryption_passphrase)?;

    // construct the client
    let client = Client::builder()
        .server_name_or_homeserver_url(account_data.user_id.server_name())
        .sqlite_store(
            sqlite_path,
            Some(&encryption_passphrase), // same as for encrypted files
        )
        .build()
        .await?;

    // restore session from the unified account struct
    client
        .restore_session(Account {
            data: account_data.clone(),
            secure_data: secure_account_data,
        })
        .await?;

    Ok(Some(client))
}

/// Tries to log a user in with the provided homeserver, username and password.
///
/// Also saves the new data (users.toml, encrypted file, keyring entry)
/// On success, an authenticated Client is returned.
pub async fn login_username(
    homeserver: String,
    username: String,
    password: String,
) -> anyhow::Result<Client> {
    // first remove possible leftovers
    remove_orphaned_accounts();

    // get id and passphrase
    let (id, encryption_passphrase) = generate_account_credentials();

    // define paths
    let account_path = utils::unwrap_lock(&ACCOUNT_PATH);

    let sqlite_path = account_path.join(&id);

    tokio::fs::create_dir_all(&account_path).await?;

    // construct client
    let client = Client::builder()
        .server_name_or_homeserver_url(homeserver)
        .sqlite_store(&sqlite_path, Some(&encryption_passphrase))
        .build()
        .await?;

    // start login
    let response = client
        .matrix_auth()
        .login_username(&username, &password)
        .initial_device_display_name(&utils::unwrap_lock(&INITIAL_DEVICE_NAME))
        .request_refresh_token()
        .await?;

    // construct new secure account data from response
    let secure_data = SecureAccountData::new(
        response.access_token,
        response.refresh_token,
        response.device_id,
        None,
    );

    tokio::task::spawn_blocking(move || {
        save_new_account(&id, response.user_id, &secure_data, &encryption_passphrase)
    })
    .await??;

    Ok(client)
}

/// Tries to log a user in via their homeserver.
///
/// Also saves the new data (users.toml, encrypted file, keyring entry)
/// On success, an authenticated Client is returned.
pub async fn login_sso(
    homeserver: String,
    tx: mpsc::UnboundedSender<String>,
) -> anyhow::Result<Client> {
    // first remove possible leftovers
    remove_orphaned_accounts();

    // initialize rng for later usage
    let (id, encryption_passphrase) = generate_account_credentials();

    // define the paths once
    let account_path = utils::unwrap_lock(&ACCOUNT_PATH);

    let sqlite_path = account_path.join(&id);

    tokio::fs::create_dir_all(&account_path).await?;

    // construct the client
    let client = Client::builder()
        .server_name_or_homeserver_url(homeserver)
        .sqlite_store(&sqlite_path, Some(&encryption_passphrase))
        .build()
        .await?;

    // start sso login
    let response = client
        .matrix_auth()
        .login_sso(|sso_url| async move {
            if webbrowser::open(&sso_url).is_ok() {
                tx.send("Go to the opened website to authenticate".to_string())
                    .ok();
            } else {
                tx.send(format!("Navigate to {sso_url} in a browser of choice"))
                    .ok();
            }
            Ok(())
        })
        .initial_device_display_name(&utils::unwrap_lock(&INITIAL_DEVICE_NAME))
        .request_refresh_token()
        .await?;

    // construct new secure account data from response
    let secure_data = SecureAccountData::new(
        response.access_token,
        response.refresh_token,
        response.device_id,
        None,
    );

    tokio::task::spawn_blocking(move || {
        save_new_account(&id, response.user_id, &secure_data, &encryption_passphrase)
    })
    .await??;

    Ok(client)
}

fn remove_orphaned_accounts() {
    let account_path = utils::unwrap_lock(&ACCOUNT_PATH);
    let users_path = account_path.join("users.toml");

    if !users_path.exists() {
        return;
    }

    // - collect vector of:
    //  - sqlite folders
    //  - encrypted files
    //  - accounts
    //  - keyring entries
    let mut resources = Vec::new();

    // get the user list
    let Ok(toml_account_data) = fs::read_to_string(&users_path) else {
        return;
    };

    let mut accounts: AccountList = match toml::from_str(&toml_account_data) {
        Ok(data) => data,
        Err(_) => return,
    };

    resources.extend(
        accounts
            .accounts
            .iter()
            .map(|account| AccountResource::User(account.id.clone())),
    );

    for entry in match fs::read_dir(&account_path) {
        Ok(entries) => entries,
        Err(_) => return,
    }
    .flatten()
    {
        // collect everything into resources vec
        let path = entry.path();

        // filter out only the folders
        if path.is_dir()
            && let Some(name) = entry.file_name().to_str().map(str::to_owned)
        {
            resources.push(AccountResource::Folder(name, path.clone()));
        }

        // filter out only the files except the users.toml file (so it doesnt get deleted)
        if path.is_file()
            && path != users_path
            && let Some(name) = path.file_prefix().unwrap().to_str().map(str::to_owned)
        {
            resources.push(AccountResource::File(name, path));
        }
    }

    match Entry::search(&[("service", APP_NAME)].into()) {
        Ok(entries) => resources.extend(
            entries
                .iter()
                .filter_map(|entry| Some(AccountResource::KeyringEntry(entry.get_specifiers()?.1))),
        ),
        Err(_) => return,
    }

    // all values in resource vector now

    // hashmap of all seen (if all are seen: 1111, each bit represents one variant seen)
    let mut seen: HashMap<&str, u8> = HashMap::new();

    for resource in &resources {
        let (name, bit) = match resource {
            AccountResource::User(s) => (s.as_str(), USER),
            AccountResource::File(s, _) => (s.as_str(), FILE),
            AccountResource::Folder(s, _) => (s.as_str(), FOLDER),
            AccountResource::KeyringEntry(s) => (s.as_str(), KEYRING),
        };

        *seen.entry(name).or_default() |= bit;
    }

    for resource in &resources {
        let name = match resource {
            AccountResource::User(s)
            | AccountResource::File(s, _)
            | AccountResource::Folder(s, _)
            | AccountResource::KeyringEntry(s) => s.as_str(),
        };

        if let Some(mask) = seen.get(name)
            && *mask != ALL
        {
            match resource {
                AccountResource::File(_, path) => {
                    // delete file
                    fs::remove_file(path).ok();
                }

                AccountResource::Folder(_, path) => {
                    // delete folder
                    fs::remove_dir_all(path).ok();
                }

                AccountResource::KeyringEntry(name) => {
                    // delete keyring entry
                    let Ok(entry) = Entry::new(APP_NAME, name) else {
                        return;
                    };
                    entry.delete_credential().ok();
                }

                AccountResource::User(s) => {
                    // remove user from users.toml
                    accounts.accounts.retain(|account| &account.id != s);
                }
            }
        }
    }

    let Ok(toml_account_data) = toml::to_string(&accounts) else {
        return;
    };

    fs::write(&users_path, toml_account_data).ok();
}

pub fn set_active_account(user_id: &str, active: bool) -> anyhow::Result<()> {
    let account_path = utils::unwrap_lock(&ACCOUNT_PATH);
    let users_path = account_path.join("users.toml");

    let mut accounts = load_account_list(&users_path)?;

    // check if account exists
    accounts
        .accounts
        .iter()
        .any(|account| account.user_id == user_id)
        .ok_or_else(|| anyhow::anyhow!("User not found: {}", user_id))?;

    accounts.accounts.iter_mut().for_each(|account| {
        if account.user_id == user_id {
            account.active = active;
        } else {
            account.active = false;
        }
    });

    let toml_account_data = toml::to_string(&accounts)?;

    fs::write(&users_path, toml_account_data)?;

    Ok(())
}

fn update_account_config(
    user_id: &str,
    access_token: Option<String>,
    refresh_token: Option<Option<String>>,
) -> anyhow::Result<()> {
    let account_path = utils::unwrap_lock(&ACCOUNT_PATH);
    let users_path = account_path.join("users.toml");

    let accounts = load_account_list(&users_path)?;

    let id = accounts
        .accounts
        .iter()
        .find(|account| account.user_id == user_id)
        .ok_or_else(|| anyhow::anyhow!("User not found: {}", user_id))?
        .id
        .clone();

    let secure_path = account_path.join(format!("{}.enc", id));

    let encryption_passphrase = load_encryption_passphrase(&id)?;

    let mut secure_data = load_secure_account_data(&secure_path, &encryption_passphrase)?;

    if let Some(access_token) = access_token {
        secure_data.access_token = access_token;
    }
    if let Some(refresh_token) = refresh_token {
        secure_data.refresh_token = refresh_token;
    }

    // secure_data struct -> toml
    let serialized = toml::to_string(&secure_data)?;

    // get recipient from passphrase
    let recipient = age::scrypt::Recipient::new(SecretString::from(encryption_passphrase));

    // encrypt secure data
    let encrypted_bytes = age::encrypt(&recipient, serialized.as_bytes())?;

    // write bytes to encrypted file
    atomic_write(&secure_path, &encrypted_bytes)?;

    Ok(())
}

pub fn remove_account_config(user_id: &str) {
    let account_path = utils::unwrap_lock(&ACCOUNT_PATH);
    let users_path = account_path.join("users.toml");

    let Ok(mut accounts) = load_account_list(&users_path) else {
        return;
    };

    let Some(account) = accounts
        .accounts
        .iter()
        .find(|account| account.user_id == user_id)
    else {
        return;
    };

    let account_id = account.id.clone();

    let dir_path = account_path.join(&account_id);

    if !dir_path.exists() {
        return;
    }

    let enc_path = account_path.join(format!("{}.enc", account_id));

    if !enc_path.exists() {
        return;
    }

    let Ok(entry) = Entry::new(APP_NAME, &account_id) else {
        return;
    };

    fs::remove_dir_all(dir_path).ok();
    fs::remove_file(enc_path).ok();
    entry.delete_credential().ok();
    accounts.accounts.retain(|a| a.id != account_id);

    let Ok(toml_account_data) = toml::to_string(&accounts) else {
        return;
    };

    fs::write(&users_path, toml_account_data).ok();
}

fn atomic_write<C: AsRef<[u8]>>(target: &Path, contents: C) -> anyhow::Result<()> {
    let tmp = target.with_extension("tmp");
    fs::write(&tmp, contents)?;
    fs::rename(&tmp, target)?;
    Ok(())
}

fn load_account_list(path: &Path) -> anyhow::Result<AccountList> {
    if !path.exists() {
        return Ok(AccountList::default());
    }

    let toml_account_data = fs::read_to_string(path)?;
    toml::from_str(&toml_account_data).map_err(|e| anyhow::anyhow!(e))
}

fn load_secure_account_data(
    path: &Path,
    encryption_passphrase: &str,
) -> anyhow::Result<SecureAccountData> {
    // load encrypted file contents
    let data = fs::read(path)?;

    // get identity from passphrase
    let identity = age::scrypt::Identity::new(SecretString::from(encryption_passphrase));
    let decrypted_bytes = age::decrypt(&identity, &data)?;
    toml::from_slice(&decrypted_bytes).map_err(|e| anyhow::anyhow!(e))
}

fn active_account(accounts: &[AccountData]) -> Option<&AccountData> {
    // filter the accounts for only active accounts
    let mut active_accounts = accounts.iter().filter(|a| a.active);
    // if multiple, no account is active
    match (active_accounts.next(), active_accounts.next()) {
        (Some(account), None) => Some(account),
        _ => None,
    }
}

fn load_encryption_passphrase(id: &str) -> Result<String, keyring_core::Error> {
    // retrieve encryption passphrase from keyring (used for file encryption and db encryption)
    let encryption_passphrase_entry = Entry::new(APP_NAME, id)?;
    encryption_passphrase_entry.get_password()
}

fn generate_account_credentials() -> (String, String) {
    let mut rng = rand::rng();
    // generate id for usage on sqlite store and other files
    let id = Alphanumeric.sample_string(&mut rng, 32);
    // generate passphrase for sqlite store and encrypted files
    let encryption_passphrase = Alphanumeric.sample_string(&mut rng, 32);
    (id, encryption_passphrase)
}

fn save_new_account(
    id: &str,
    user_id: OwnedUserId,
    secure_data: &SecureAccountData,

    encryption_passphrase: &str,
) -> anyhow::Result<()> {
    let account_path = utils::unwrap_lock(&ACCOUNT_PATH);

    let backup_path = account_path.join("users.toml.backup");
    let users_path = account_path.join("users.toml");
    let secure_path = account_path.join(format!("{id}.enc"));
    let sqlite_path = account_path.join(id);

    // secure_data struct -> toml
    let serialized = toml::to_string(secure_data)?;

    // get recipient from passphrase
    let recipient = age::scrypt::Recipient::new(SecretString::from(encryption_passphrase));

    // encrypt secure data
    let encrypted_bytes = age::encrypt(&recipient, serialized.as_bytes())?;

    let encryption_passphrase_entry = Entry::new(APP_NAME, id)?;

    // create guard as soon as possible
    let mut guard = AccountCreationGuard {
        backup_path: backup_path.clone(),
        backup_tmp_path: backup_path.with_extension("tmp").clone(),
        backup_created: false,

        users_path: users_path.clone(),

        sqlite_path: sqlite_path.clone(),
        secure_path: secure_path.clone(),

        users_tmp_path: users_path.with_extension("tmp").clone(),
        secure_tmp_path: secure_path.with_extension("tmp").clone(),

        keyring_entry: encryption_passphrase_entry,
        keyring_created: false,

        committed: false,
    };

    // read unencrypted file with all users
    let mut accounts: AccountList = if users_path.exists() {
        let toml_account_data = fs::read_to_string(&users_path)?;
        toml::from_str(&toml_account_data)?
    } else {
        AccountList::default()
    };

    // create backup of accounts
    let toml_account_data = toml::to_string(&accounts)?;
    atomic_write(&backup_path, &toml_account_data)?;

    guard.backup_created = true;

    // set all accounts active to false (for the new account to be active)
    accounts.accounts.iter_mut().for_each(|a| a.active = false);

    // add new account to vector
    accounts.accounts.push(AccountData {
        id: id.to_string(),
        user_id,
        active: true,
    });

    let toml_account_data = toml::to_string(&accounts)?;

    // write bytes to encrypted file
    atomic_write(&secure_path, &encrypted_bytes)?;

    // write unencrypted file
    atomic_write(&users_path, toml_account_data)?;

    // save encryption passphrase to keyring (used for file encryption and db encryption)
    guard.keyring_entry.set_password(encryption_passphrase)?;

    guard.keyring_created = true;

    let _ = fs::remove_file(&backup_path);

    guard.commit();

    Ok(())
}

impl From<Account> for AuthSession {
    fn from(account: Account) -> Self {
        if let Some(client_id) = account.secure_data.client_id {
            OAuthSession {
                client_id,
                user: UserSession {
                    meta: SessionMeta {
                        user_id: account.data.user_id,
                        device_id: account.secure_data.device_id,
                    },
                    tokens: SessionTokens {
                        access_token: account.secure_data.access_token,
                        refresh_token: account.secure_data.refresh_token,
                    },
                },
            }
            .into()
        } else {
            MatrixSession {
                meta: SessionMeta {
                    user_id: account.data.user_id,
                    device_id: account.secure_data.device_id,
                },
                tokens: SessionTokens {
                    access_token: account.secure_data.access_token,
                    refresh_token: account.secure_data.refresh_token,
                },
            }
            .into()
        }
    }
}
