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

use super::{
    Account, SecureAccountData, active_account, generate_account_credentials, load_account_list,
    load_encryption_passphrase, load_secure_account_data, remove_orphaned_accounts,
    save_new_account,
};
use crate::{ACCOUNT_PATH, INITIAL_DEVICE_NAME, utils};
use matrix_sdk::Client;
use tokio::sync::mpsc;

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

// WARNING: deprecated (soon)
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
