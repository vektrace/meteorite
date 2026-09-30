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

use crate::{ACCOUNT_PATH, APP_NAME, utils};
use keyring_core::Entry;
use matrix_sdk::{
    AuthSession, SessionMeta, SessionTokens,
    authentication::{
        matrix::MatrixSession,
        oauth::{ClientId, OAuthSession, UserSession},
    },
    ruma::{OwnedDeviceId, OwnedUserId},
};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

pub mod auth;

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

pub fn remove_account(user_id: &str) {
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

fn load_account_list(path: &Path) -> anyhow::Result<AccountList> {
    if !path.exists() {
        return Ok(AccountList::default());
    }

    let toml_account_data = fs::read_to_string(path)?;
    toml::from_str(&toml_account_data).map_err(|e| anyhow::anyhow!(e))
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
