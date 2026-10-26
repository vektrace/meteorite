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

use super::components;
use dioxus::prelude::*;

#[component]
pub fn FatalError(message: String) -> Element {
    rsx! {
        components::Bg {
            div {
                div {
                    class: "fixed top-12 left-1/2 -translate-x-1/2 z-50 flex flex-col items-center transition-opacity duration-300 ease-in-out opacity-100 animate-fade-in",

                    div {
                        class: "flex items-center justify-center",
                        components::Icon {
                            size: "w-64 h-64",
                        }
                    }

                    div {
                        class: "h-12",
                    }

                    h1 {
                        class: "text-white text-3xl text-center",
                        "Unable to start the application"
                    }

                    p {
                        class: "text-neutral-300 text-xl text-center my-4",
                        "{message}"
                    }

                    p {
                        class: "text-neutral-300 text-lg text-center",
                        "Need help? Ask for help or "
                        a {
                            class: "text-cyan-400 hover:text-cyan-500",
                            href: "#",
                            onclick: move |evt| {
                                evt.prevent_default();
                                let _ = webbrowser::open("https://github.com/vektrace/meteorite/issues");
                            },
                            "open an issue on GitHub"
                        }
                        " and include the error details."
                    }
                }
                components::Footer {}
            }
        }
    }
}
