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

use matrix_sdk::{
    AuthSession, SessionMeta, SessionTokens,
    authentication::{
        matrix::MatrixSession,
        oauth::{ClientId, OAuthSession, UserSession},
    },
    ruma::{OwnedDeviceId, OwnedUserId},
};
use serde::{Deserialize, Serialize};

pub mod auth;

// toml data wrapper
#[derive(Default, Deserialize, Serialize, Clone)]
pub(crate) struct AccountList {
    pub accounts: Vec<AccountData>,
}

impl From<&[AccountData]> for AccountList {
    fn from(accounts: &[AccountData]) -> Self {
        AccountList {
            accounts: accounts.to_vec(),
        }
    }
}

impl From<AccountList> for Vec<AccountData> {
    fn from(accounts: AccountList) -> Self {
        accounts.accounts
    }
}

impl AccountList {
    fn active(&self) -> Option<&AccountData> {
        // filter the accounts for only active accounts
        let mut active_accounts = self.accounts.iter().filter(|a| a.active);
        // if multiple, no account is active
        match (active_accounts.next(), active_accounts.next()) {
            (Some(account), None) => Some(account),
            _ => None,
        }
    }
}

// used for restoring sessions
pub(crate) struct Account {
    pub data: AccountData,
    pub secure_data: SecureAccountData,
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
pub(crate) struct AccountData {
    // id used for files instead of user_id
    pub id: String,
    pub user_id: OwnedUserId,
    pub active: bool,
}

// stored in encrypted file, identity stored in keyring
// only loaded when required
#[derive(Deserialize, Serialize)]
pub(crate) struct SecureAccountData {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub device_id: OwnedDeviceId,
    // only present when logged in via oauth
    pub client_id: Option<ClientId>,
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
