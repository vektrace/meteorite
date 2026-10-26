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

use super::{CLIENT, components, login};
use dioxus::prelude::*;
use meteorite_core::account::auth;

#[component]
pub fn LoadingScreen() -> Element {
    let mut starts_fading = use_signal(|| false);
    let mut error = use_signal(|| Option::<String>::None);
    let mut login = use_signal(|| false);
    let mut retry = use_signal(|| 0);

    let _login = use_resource(move || {
        let _ = retry();

        async move {
            let handle = tokio::spawn(async move { auth::login().await });

            match handle.await {
                Ok(Ok(Some(client))) => {
                    starts_fading.set(true);
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                    *CLIENT.write() = Some(client);
                }
                Ok(Ok(None)) => {
                    login.set(true);
                }
                Ok(Err(e)) => {
                    error.set(Some(e.to_string()));
                }
                Err(_) => {
                    // *should* not happen
                    error.set(Some("Task failed".to_string()));
                }
            }
        }
    });

    let opacity = if starts_fading() && error.read().is_none() {
        "opacity-0"
    } else {
        "opacity-100"
    };

    if *login.read() {
        rsx! {
            login::LoginScreen {}
        }
    } else {
        rsx! {
            components::Bg {
                div {
                    class: "{opacity} transition-opacity duration-250 ease-in-out",
                    div {
                        class: "flex flex-col items-center min-h-screen relative",
                        div {
                            class: "mt-12",
                            components::Icon {
                                size: "h-64 w-64",
                            }
                        }

                        if let Some(e) = &*error.read() {
                            div {
                                class: "h-12",
                            }

                            p {
                                class: "p-4 rounded-xl bg-red-400 border-3 border-red-600 text-neutral-800 font-medium shadow-lg text-center",
                                "{e}"
                            }

                            div {
                                class: "h-12",
                            }

                            button {
                                class: "w-1/5 py-2 bg-red-600 hover:bg-red-500 rounded-lg text-white font-medium transition-colors cursor-pointer",
                                onclick: move |_| {
                                    error.set(None);
                                    starts_fading.set(false);
                                    retry += 1;
                                },
                                "Retry"
                            }
                        } else {
                            div {
                                class: "text-center mt-16",
                                components::Spinner {
                                    size: "h-32 w-32",
                                }
                            }
                        }
                    }
                    components::Footer {}
                }
            }
        }
    }
}
