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

use dioxus::prelude::*;

#[component]
pub fn Bg(children: Element) -> Element {
    rsx! {
        div {
            class: "fixed inset-0 bg-gradient-to-b from-neutral-900 to-indigo-950 z-0 overflow-hidden",
            div {
                class: "relative z-10 w-full h-full",
                {children}
            }
        }
    }
}
