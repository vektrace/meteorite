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

use dioxus::desktop::{Config, WindowBuilder};
use dioxus::prelude::*;
use meteorite_core::Client;
use meteorite_core::{base_path, init};

mod components;
mod views;

use views::{error, main};

#[derive(PartialEq, Clone, Copy, Default)]
enum LoginStage {
    #[default]
    Homeserver,
    Credentials,
}

const ICON: Asset = asset!("/assets/icon/icon.png");

const MAIN_CSS: Asset = asset!("/assets/styling/main.css");
const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");

static CLIENT: GlobalSignal<Option<Client>> = Signal::global(|| None::<Client>);

#[tokio::main]
async fn main() {
    init::setup_device_name();
    init::setup_folders();

    // TODO: set icon
    LaunchBuilder::new()
        .with_cfg(desktop! {
            Config::default()
                .with_data_directory(base_path())
                .with_menu(None)
                .with_window(WindowBuilder::new().with_title("meteorite"))
        })
        .launch(App);
}

#[component]
fn App() -> Element {
    let keyring_error = init::setup_keyring().err();
    let _keyring_guard = use_signal(|| meteorite_core::KeyringGuard);

    rsx! {
        // TODO: adjust title based on what the user is doing, e.g. (3) meteorite - Matrix HQ
        document::Title {
            "meteorite"
        }
        document::Link {
            rel: "stylesheet",
            href: MAIN_CSS,
        }
        document::Link {
            rel: "stylesheet",
            href: TAILWIND_CSS,
        }

        div {
            class: "app-container font-sans text-base antialiased bg-neutral-900 text-white min-h-screen p-4",

            if let Some(e) = keyring_error {
                error::FatalError { message: format!("The application failed to set up the keyring store.\n\nDetails: {e}")}
            }

            main::MainScreen {}
        }
    }
}
