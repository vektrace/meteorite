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

use crate::{APP_NAME, INITIAL_DEVICE_NAME};
use std::fs;

// all functions in this file are used to inititalize something (e.g. set default keyring store)

/// Sets the initial device display name based on which OS the user is on.
pub fn setup_device_name() {
    let name = if cfg!(target_os = "windows") {
        "Windows"
    } else if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(target_os = "linux") {
        "Linux"
    } else if cfg!(target_os = "ios") {
        "iOS"
    } else if cfg!(target_os = "android") {
        "Android"
    } else {
        "Unknown"
    };

    INITIAL_DEVICE_NAME
        .set(format!("meteorite Client ({})", name))
        .unwrap();
}

/// Sets the default keyring store.
pub fn setup_keyring() -> Result<(), keyring_core::Error> {
    #[cfg(target_os = "linux")]
    keyring_core::set_default_store(zbus_secret_service_keyring_store::Store::new()?);
    #[cfg(target_os = "windows")]
    keyring_core::set_default_store(windows_native_keyring_store::Store::new()?);
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    // protected requires code-signed application (which we should do anyways, possibly just use self-signed one to avoid paying)
    keyring_core::set_default_store(apple_native_keyring_store::protected::Store::new()?);
    #[cfg(target_os = "android")]
    keyring_core::set_default_store(android_native_keyring_store::Store::new()?);

    Ok(())
}

/// Creates by the application required folders.
#[cfg(not(target_family = "wasm"))]
pub fn setup_folders() {
    use crate::storage::native::{ACCOUNT_PATH, BASE_PATH};

    let base_path = sysdirs::data_local_dir().unwrap().join(APP_NAME);
    let account_path = base_path.join("accounts");

    // create dirs on first run
    if !base_path.exists() {
        // should only panic if perms are set wrong
        fs::create_dir_all(&base_path).unwrap_or_else(|e| {
            panic!(
                "failed to create application data directory {:?}: {e}",
                base_path
            )
        });
    }
    if !account_path.exists() {
        fs::create_dir_all(&account_path).unwrap_or_else(|e| {
            panic!(
                "failed to create application data directory {:?}: {e}",
                account_path
            )
        });
    }

    // set() can only return an error when it has already been set, which in this case cannot
    // happen.
    BASE_PATH.set(base_path).unwrap();
    ACCOUNT_PATH.set(account_path).unwrap();
}
