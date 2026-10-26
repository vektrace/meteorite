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

use super::{CLIENT, loading};
use dioxus::prelude::*;

#[component]
pub fn MainScreen() -> Element {
    if CLIENT().is_none() {
        rsx! {
            loading::LoadingScreen {}
        }
    } else {
        rsx! {
            div {
                class: "flex h-screen w-screen font-sans m-0 p-0",
                div {
                    class: "w-[75px] min-w-[75px] flex flex-col items-center border-r border-neutral-600",

                    div {
                        class: "h-[5px]",
                    }

                    button {
                        onclick: move |_| println!("home"),
                        class: "w-12 h-12 rounded-2xl m-1 p-0 bg-none overflow-hidden border border-neutral-600",

                        img {
                            src: asset!("/assets/home.png"),
                            width: "50",
                            height: "50",
                            class: "block",
                        }
                    }

                    hr {
                        class: "w-[80%] mx-[10px] my-0 border-t border-neutral-600",
                    }
                }

                div {
                    class: "grow shrink basis-0 p-4",
                    h1 {
                        "meteorite"
                    }
                    button {
                        class: "border border-neutral-600 rounded-xl p-1",
                        onclick: move |_| println!("clicked button"),
                        "test"
                    }
                }

                div {
                    class: "w-[350px] min-w-[350px] p-4 border-l border-neutral-600",
                    button {
                        class: "border border-neutral-600 rounded-xl p-1",
                        onclick: move |_| println!("settings"),
                        "settings"
                    }
                }
            }
        }
    }
}
