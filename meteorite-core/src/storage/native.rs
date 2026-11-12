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

use super::{Result, StorageApi, StorageError};
use crate::{
    APP_NAME,
    account::{Account, AccountData, AccountList, SecureAccountData},
};
use age::{secrecy::ExposeSecret, x25519::Identity};
use keyring_core::Entry;
use matrix_sdk::{Client, ClientBuilder, SqliteStoreConfig, ruma::UserId};
use rand::distr::{Alphanumeric, SampleString};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::OnceLock;

pub(crate) static BASE_PATH: OnceLock<PathBuf> = OnceLock::new();
pub(crate) static ACCOUNT_PATH: OnceLock<PathBuf> = OnceLock::new();

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

/*
#[derive(Debug, Error)]
pub(crate) enum BackendError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("toml serialize error: {0}")]
    TomlSer(#[from] toml::ser::Error),
    #[error("toml deserialize error: {0}")]
    TomlDe(#[from] toml::de::Error),
    #[error("encrypt error: {0}")]
    Encrypt(#[from] age::EncryptError),
    #[error("decrypt error: {0}")]
    Decrypt(#[from] age::DecryptError),
    #[error("identity parse error: {0}")]
    IdentityParse(&'static str),
    #[error("keyring error: {0}")]
    Keyring(#[from] keyring_core::Error),
}
*/

/// Implementation of the storage API for native targets (mobile, desktop).
pub(crate) struct Native {
    identity: Identity,
    id: String,
}

impl StorageApi for Native {
    fn new() -> Self {
        let mut rng = rand::rng();
        // generate id
        let id = Alphanumeric.sample_string(&mut rng, 32);
        // generate x25519 identity
        let identity = Identity::generate();
        Self { id, identity }
    }

    async fn load(id: &str) -> Result<(Self, Account)> {
        let account_path = ACCOUNT_PATH
            .get()
            .expect("account path should be initialized");
        let secure_path = account_path.join(format!("{}.enc", id));

        let accounts: Vec<AccountData> = Self::account_list().await?.into();
        let account_data = accounts
            .into_iter()
            .find(|a| a.id == id)
            .ok_or(StorageError::NoAccount(id.to_string()))?;

        let entry = Entry::new(APP_NAME, id).map_err(|e| StorageError::Backend(Box::new(e)))?;
        let raw = entry.get_password().map_err(|e| match e {
            keyring_core::Error::NoEntry => StorageError::NoAccount(id.to_string()),
            other => StorageError::Backend(Box::new(other)),
        })?;
        let identity = Identity::from_str(raw.trim())
            .map_err(|e| StorageError::Backend(Box::new(std::io::Error::other(e))))?;

        // load encrypted file contents
        let data = fs::read(secure_path).map_err(|e| StorageError::Backend(Box::new(e)))?;

        let decrypted_bytes =
            age::decrypt(&identity, &data).map_err(|e| StorageError::Backend(Box::new(e)))?;
        let secure_data =
            toml::from_slice(&decrypted_bytes).map_err(|e| StorageError::Backend(Box::new(e)))?;

        Ok((
            Self {
                identity,
                id: id.to_string(),
            },
            Account {
                data: account_data,
                secure_data,
            },
        ))
    }

    async fn account_list() -> Result<AccountList> {
        let path = ACCOUNT_PATH
            .get()
            .expect("account path should be initialized")
            .join("users.toml");

        if !path.exists() {
            return Ok(AccountList::default());
        }

        let data = fs::read_to_string(path).map_err(|e| StorageError::Backend(Box::new(e)))?;
        toml::from_str(&data).map_err(|e| StorageError::Backend(Box::new(e)))
    }

    /// Deletion on a best-effort basis
    async fn remove(id: &str) -> Result<()> {
        let account_path = ACCOUNT_PATH
            .get()
            .expect("account path should be initialized");
        let users_path = account_path.join("users.toml");

        let mut accounts = Self::account_list().await?;

        let dir_path = account_path.join(id);
        let enc_path = account_path.join(format!("{}.enc", id));

        // should only fail on lacking perms
        // we ignore and keep going (best effort deletion)
        if dir_path.exists() {
            fs::remove_dir_all(dir_path).ok();
        }

        if enc_path.exists() {
            fs::remove_file(enc_path).ok();
        }

        if let Ok(entry) = Entry::new(APP_NAME, id) {
            entry.delete_credential().ok();
        }

        accounts.accounts.retain(|a| a.id != id);

        if let Ok(account_data) = toml::to_string(&accounts) {
            atomic_write(&users_path, account_data).ok();
        }

        Ok(())
    }

    async fn cleanup() {
        let account_path = ACCOUNT_PATH
            .get()
            .expect("account path should be initialized");
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
        let Ok(mut accounts) = Self::account_list().await else {
            return;
        };

        resources.extend(
            accounts
                .accounts
                .iter()
                .map(|account| AccountResource::User(account.id.clone())),
        );

        for entry in match fs::read_dir(account_path) {
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

            // filter out only the files except the users.toml file
            if path.is_file()
                && path != users_path
                && let Some(name) = path.file_prefix().unwrap().to_str().map(str::to_owned)
            {
                resources.push(AccountResource::File(name, path));
            }
        }

        match Entry::search(&[("service", APP_NAME)].into()) {
            Ok(entries) => resources.extend(entries.iter().filter_map(|entry| {
                Some(AccountResource::KeyringEntry(entry.get_specifiers()?.1))
            })),
            Err(_) => return,
        }

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

        let complete: HashSet<&str> = seen
            .iter()
            .filter(|(_, mask)| **mask == ALL)
            .map(|(name, _)| *name)
            .collect();

        for resource in &resources {
            let name = match resource {
                AccountResource::User(s)
                | AccountResource::File(s, _)
                | AccountResource::Folder(s, _)
                | AccountResource::KeyringEntry(s) => s.as_str(),
            };

            if complete.contains(name) {
                continue;
            }

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
                    if let Ok(entry) = Entry::new(APP_NAME, name) {
                        entry.delete_credential().ok();
                    };
                }

                AccountResource::User(_) => {
                    // removal happens later
                }
            }
        }

        // remove all incomplete accounts from users.toml
        accounts
            .accounts
            .retain(|a| complete.contains(a.id.as_str()));

        let Ok(account_data) = toml::to_string(&accounts) else {
            return;
        };

        atomic_write(&users_path, account_data).ok();
    }

    async fn update_tokens(
        id: &str,
        access_token: Option<String>,
        refresh_token: Option<Option<String>>,
    ) -> Result<()> {
        let (backend, mut account) = Self::load(id).await?;

        let account_path = ACCOUNT_PATH
            .get()
            .expect("account path should be initialized");
        let secure_path = account_path.join(format!("{}.enc", id));

        if let Some(access_token) = access_token {
            account.secure_data.access_token = access_token;
        }
        if let Some(refresh_token) = refresh_token {
            account.secure_data.refresh_token = refresh_token;
        }

        // secure_data struct -> toml
        let serialized = toml::to_string(&account.secure_data)
            .map_err(|e| StorageError::Backend(Box::new(e)))?;

        // encrypt secure data
        let encrypted_bytes = age::encrypt(&backend.identity.to_public(), serialized.as_bytes())
            .map_err(|e| StorageError::Backend(Box::new(e)))?;

        // write bytes to encrypted file
        atomic_write(&secure_path, &encrypted_bytes)
            .map_err(|e| StorageError::Backend(Box::new(e)))?;

        Ok(())
    }

    async fn user_id_to_id(user_id: &UserId) -> Result<String> {
        let accounts: Vec<AccountData> = Self::account_list().await?.into();
        Ok(accounts
            .into_iter()
            .find(|a| a.user_id == user_id)
            .ok_or(StorageError::NoAccount(user_id.to_string()))?
            .id)
    }

    async fn save(&self, user_id: &UserId, secure_data: &SecureAccountData) -> Result<()> {
        let account_path = ACCOUNT_PATH
            .get()
            .expect("account path should be initialized");

        let users_path = account_path.join("users.toml");
        let secure_path = account_path.join(format!("{}.enc", self.id));

        // secure_data struct -> toml
        let serialized =
            toml::to_string(secure_data).map_err(|e| StorageError::Backend(Box::new(e)))?;

        // encrypt secure data
        let encrypted_bytes = age::encrypt(&self.identity.to_public(), serialized.as_bytes())
            .map_err(|e| StorageError::Backend(Box::new(e)))?;

        let entry =
            Entry::new(APP_NAME, &self.id).map_err(|e| StorageError::Backend(Box::new(e)))?;

        // read unencrypted file with all users
        let mut accounts = Self::account_list().await?;

        // set all accounts active to false (for the new account to be active)
        accounts.accounts.iter_mut().for_each(|a| a.active = false);

        let account_data = AccountData {
            id: self.id.clone(),
            user_id: user_id.into(),
            active: true,
        };

        // add new account to vector
        if let Some(account) = accounts.accounts.iter_mut().find(|a| a.user_id == user_id) {
            *account = account_data;
        } else {
            accounts.accounts.push(account_data);
        }

        let account_data =
            toml::to_string(&accounts).map_err(|e| StorageError::Backend(Box::new(e)))?;

        // write bytes to encrypted file
        atomic_write(&secure_path, &encrypted_bytes)
            .map_err(|e| StorageError::Backend(Box::new(e)))?;

        // save identity to keyring
        entry
            .set_password(self.identity.to_string().expose_secret())
            .map_err(|e| StorageError::Backend(Box::new(e)))?;

        // write unencrypted file
        atomic_write(&users_path, account_data).map_err(|e| StorageError::Backend(Box::new(e)))?;

        Ok(())
    }

    fn setup_matrix_store(&self) -> ClientBuilder {
        let sqlite_path = ACCOUNT_PATH
            .get()
            .expect("account path should be initialized")
            .join(&self.id);
        let config = SqliteStoreConfig::new(&sqlite_path)
            .key(Some(self.identity.to_string().expose_secret().as_bytes()));
        Client::builder().sqlite_store_with_config_and_cache_path(config, None::<PathBuf>)
    }
}

fn atomic_write<C: AsRef<[u8]>>(target: &Path, contents: C) -> std::io::Result<()> {
    let tmp = target.with_added_extension("tmp");
    let mut f = File::create(&tmp)?;
    f.write_all(contents.as_ref())?;
    f.sync_all()?;
    fs::rename(&tmp, target)?;
    Ok(())
}
