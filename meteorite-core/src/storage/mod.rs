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

//! This module adds a unified way to save and load data, independent of the storage backend.
//!
//! It is intended to be only used for persisting data when managing accounts,for example in the
//! login flow.
//!
//! The goal is to make it easier to add or modify backends (such as web or mobile)
//! without changing any of the login functionality.

use crate::account::{Account, AccountList, SecureAccountData};
use matrix_sdk::{ClientBuilder, ruma::UserId};
use thiserror::Error;

#[cfg(not(target_family = "wasm"))]
pub(crate) mod native;

#[cfg(not(target_family = "wasm"))]
use native::Native as Impl;

pub(crate) type Result<T> = std::result::Result<T, StorageError>;

#[derive(Error, Debug)]
pub(crate) enum StorageError {
    #[error("no account found for id {0}")]
    NoAccount(String),
    #[error(transparent)]
    Backend(#[from] Box<dyn std::error::Error + Send + Sync>),
}

pub(crate) trait StorageApi: Sized {
    fn new() -> Self;
    fn load(id: &str) -> Result<(Self, Account)>;
    fn account_list() -> Result<AccountList>;
    fn remove(id: &str) -> Result<()>;
    async fn cleanup();
    fn update_tokens(
        id: &str,
        access_token: Option<String>,
        refresh_token: Option<Option<String>>,
    ) -> Result<()>;
    fn user_id_to_id(user_id: &UserId) -> Result<String>;
    fn save(&self, user_id: &UserId, secure_data: &SecureAccountData) -> Result<()>;
    fn setup_matrix_store(&self) -> ClientBuilder;
}

pub(crate) struct Core {
    backend: Impl,
}

impl StorageApi for Core {
    fn new() -> Self {
        let backend = Impl::new();
        Self { backend }
    }
    fn load(id: &str) -> Result<(Self, Account)> {
        let backend = Impl::load(id)?;
        Ok((Self { backend: backend.0 }, backend.1))
    }
    fn account_list() -> Result<AccountList> {
        Impl::account_list()
    }
    fn remove(id: &str) -> Result<()> {
        Impl::remove(id)
    }
    async fn cleanup() {
        Impl::cleanup().await;
    }
    fn update_tokens(
        id: &str,
        access_token: Option<String>,
        refresh_token: Option<Option<String>>,
    ) -> Result<()> {
        Impl::update_tokens(id, access_token, refresh_token)
    }
    fn user_id_to_id(user_id: &UserId) -> Result<String> {
        Impl::user_id_to_id(user_id)
    }
    fn save(&self, user_id: &UserId, secure_data: &SecureAccountData) -> Result<()> {
        self.backend.save(user_id, secure_data)
    }
    fn setup_matrix_store(&self) -> ClientBuilder {
        self.backend.setup_matrix_store()
    }
}
