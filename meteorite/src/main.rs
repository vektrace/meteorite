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

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use meteorite_core::Client;
use meteorite_core::{base_path, init};
use slint::SharedString;
use tokio::sync::RwLock;

mod loading;

pub static CLIENT: RwLock<Option<Client>> = RwLock::const_new(None);

slint::include_modules!();
#[tokio::main]
async fn main() -> Result<(), slint::PlatformError> {
    init::setup_device_name();
    init::setup_folders();

    let app = App::new()?;
    let weak = app.as_weak();

    let handle = tokio::runtime::Handle::current();

    // loading screen retry callback
    {
        let weak = weak.clone();
        let handle = handle.clone();
        app.on_loading_retry(move || {
            if let Some(app) = weak.upgrade() {
                app.set_loading_data(LoadingData {
                    error: SharedString::new(),
                });
            }
            handle.spawn(loading::start(weak.clone()));
        });
    }

    handle.spawn(loading::start(weak.clone()));

    app.run()?;

    // TODO: migrate away from keyring
    let keyring_error = init::setup_keyring().err();
    let _keyring_guard = meteorite_core::KeyringGuard;

    Ok(())
}
