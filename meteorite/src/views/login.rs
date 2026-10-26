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

use super::{CLIENT, LoginStage, components};
use dioxus::prelude::*;
use meteorite_core::account::auth;
use meteorite_core::account::auth::oauth::qr::{GeneratedQrState, QrState};

#[component]
pub fn LoginScreen() -> Element {
    let mut current_stage = use_signal(LoginStage::default);
    let mut displayed_stage = use_signal(LoginStage::default);
    let mut stage_visible = use_signal(|| true);

    let mut show_validation_errors = use_signal(|| false);

    let homeserver = use_signal(String::new);
    let username = use_signal(String::new);
    let password = use_signal(String::new);

    let mut error = use_signal(|| Option::<String>::None);

    // oauth
    let mut link = use_signal(|| Option::<String>::None);
    // display
    let mut generated_qr_state = use_signal(|| Option::<GeneratedQrState>::None);
    let mut check_code_tx = use_signal(|| Option::<tokio::sync::mpsc::UnboundedSender<u8>>::None);
    let mut check_code_input = use_signal(String::new);
    // scan
    let mut qr_state = use_signal(|| Option::<QrState>::None);

    let mut is_busy = use_signal(|| false);
    let mut current_task = use_signal(|| Option::<dioxus_core::Task>::None);

    let mut login_choices = use_signal(|| Option::<Vec<auth::LoginChoice>>::None);

    use_effect(move || {
        let _stage = displayed_stage();

        spawn(async move {
            tokio::task::yield_now().await;

            document::eval(
                r#"
                const window = document.getElementById("login-window");
                const content = document.getElementById("login-content");

                if (window && content) {
                    window.style.height = `${content.getBoundingClientRect().height}px`;
                }
            "#,
            );
        });
    });

    use_effect(move || {
        let target = current_stage();

        if target == displayed_stage() {
            return;
        }

        spawn(async move {
            stage_visible.set(false);

            tokio::time::sleep(std::time::Duration::from_millis(200)).await;

            displayed_stage.set(target);

            tokio::task::yield_now().await;

            stage_visible.set(true);
        });
    });

    let mut cancel_active_task = move || {
        if let Some(task) = current_task.take() {
            task.cancel();
        }
        check_code_tx.set(None);
        generated_qr_state.set(None);
        qr_state.set(None);
    };

    let mut check_homeserver = move || {
        cancel_active_task();

        error.set(None);

        is_busy.set(true);

        let hs = homeserver.read().clone();

        let task = spawn(async move {
            let handle = tokio::spawn(auth::get_login_types(hs));

            match handle.await {
                Ok(Ok(choices)) => {
                    if choices.is_empty() {
                        error.set(Some("Homeserver not supported (requires SSO or an unsupported login method)".to_string()))
                    } else {
                        login_choices.set(Some(choices));
                        current_stage.set(LoginStage::Credentials);
                    }
                }
                Ok(Err(e)) => {
                    error.set(Some(e.to_string()));
                }
                Err(_) => {
                    // *should* not happen
                    error.set(Some("Validation process aborted".into()));
                }
            }

            is_busy.set(false);
        });

        current_task.set(Some(task));
    };

    let start_username_login = move |_| {
        if is_busy() {
            return;
        }

        cancel_active_task();

        if username.read().is_empty() || password.read().is_empty() {
            show_validation_errors.set(true);
            return;
        }

        show_validation_errors.set(false);
        is_busy.set(true);
        error.set(None);

        let hs = homeserver.read().clone();
        let user = username.read().clone();
        let pass = password.read().clone();

        let task = spawn(async move {
            let handle = tokio::spawn(auth::login_username(hs, user, pass, None));

            match handle.await {
                Ok(Ok(c)) => {
                    *CLIENT.write() = Some(c);
                }
                Ok(Err(e)) => {
                    error.set(Some(e.to_string()));
                }
                Err(_) => {
                    // *should* not happen
                    error.set(Some("Authentication process aborted".into()));
                }
            }

            is_busy.set(false);
        });

        current_task.set(Some(task));
    };

    let start_oauth_login = move |_| {
        if is_busy() {
            return;
        }

        cancel_active_task();

        is_busy.set(true);
        error.set(None);

        let hs = homeserver.read().clone();

        let task = spawn(async move {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

            let mut handle = tokio::spawn(auth::oauth::login(hs, None, tx));

            loop {
                tokio::select! {
                    Some(l) = rx.recv() => {
                        link.set(Some(l));
                    }
                    res = &mut handle => {
                        match res {
                            Ok(Ok(c)) => {
                                *CLIENT.write() = Some(c);
                            }
                            Ok(Err(e)) => {
                                error.set(Some(e.to_string()));
                            }
                            Err(_) => {
                                // *should* not happen
                                error.set(Some("Authentication process aborted".into()));
                            }
                        }
                        break;
                    }
                }
            }

            is_busy.set(false);
        });

        current_task.set(Some(task));
    };

    let start_oauth_qr_scan_login = move |_| {
        if is_busy() {
            return;
        }

        cancel_active_task();

        is_busy.set(true);
        error.set(None);

        let task = spawn(async move {
            let (state_tx, mut state_rx) = tokio::sync::mpsc::unbounded_channel();

            let mut handle = tokio::spawn(auth::oauth::qr::scan(state_tx));

            loop {
                tokio::select! {
                    Some(state) = state_rx.recv() => {
                        qr_state.set(Some(state));
                    }
                    res = &mut handle => {
                        match res {
                            Ok(Ok(c)) => {
                                *CLIENT.write() = Some(c);
                            }
                            Ok(Err(e)) => {
                                error.set(Some(e.to_string()));
                            }
                            Err(_) => {
                                // *should* not happen
                                error.set(Some("Authentication process aborted".into()));
                            }
                        }
                        break;
                    }
                }
            }

            is_busy.set(false);
        });

        current_task.set(Some(task));
    };

    let start_oauth_qr_display_login = move |_| {
        if is_busy() {
            return;
        }

        cancel_active_task();

        is_busy.set(true);
        error.set(None);

        let hs = homeserver.read().clone();

        let task = spawn(async move {
            let (state_tx, mut state_rx) = tokio::sync::mpsc::unbounded_channel();
            let (code_tx, code_rx) = tokio::sync::mpsc::unbounded_channel();

            check_code_tx.set(Some(code_tx));

            let mut handle = tokio::spawn(auth::oauth::qr::display(hs, state_tx, code_rx));

            loop {
                tokio::select! {
                    Some(state) = state_rx.recv() => {
                        generated_qr_state.set(Some(state));
                    }
                    res = &mut handle => {
                        match res {
                            Ok(Ok(c)) => {
                                *CLIENT.write() = Some(c);
                            }
                            Ok(Err(e)) => {
                                error.set(Some(e.to_string()));
                            }
                            Err(_) => {
                                // *should* not happen
                                error.set(Some("Authentication process aborted".into()));
                            }
                        }
                        break;
                    }
                }
            }

            check_code_tx.set(None);
            is_busy.set(false);
        });

        current_task.set(Some(task));
    };

    rsx! {
        components::Bg {
            div {
                class: "relative, min-h-screen flex flex-col items-center justify-center p-4",
                if let Some(err_msg) = error() {
                    div {
                        class: "absolute top-12 z-50 w-full max-w-md p-4 rounded-lg bg-red-400 border-2 border-red-600 text-neutral-800 text-center shadow-lg transition-all duration-300 animate-fade-in",
                        "{err_msg}"
                    }
                }

                if is_busy() {
                    div { class: "absolute inset-0 z-30 bg-black/40 backdrop-blur-sm" }

                    if let Some(link) = link() {
                        div {
                            class: "absolute top-24 z-40 w-full max-w-md p-3 rounded-lg bg-neutral-900 border border-neutral-700 text-white text-center shadow-xl transition-all duration-300 animate-fade-in",
                            "{link}"
                        }
                    }

                    if let Some(state) = generated_qr_state.read().as_ref() {
                        match state {
                            GeneratedQrState::WaitingForScan(data) => {
                                rsx! {
                                    div {
                                        class: "absolute top-24 z-40 w-full max-w-lg p-3 rounded-lg bg-neutral-900 border border-neutral-700 shadow-xl transition-all duration-300 animate-fade-in",
                                        p {
                                            class: "text-white text-xl text-center font-medium mb-4",
                                            "Scan the QR code with an authenticated device"
                                        }
                                        div {
                                            class: "flex justify-center items-center z-40 w-fit mx-auto bg-white p-4 rounded-md [&>svg]:block [&>svg]:w-auto [&>svg]:max-w-full [&>svg]:h-auto ",
                                            dangerous_inner_html: "{data}"
                                        }
                                    }
                                }
                            },
                            GeneratedQrState::WaitingForCheckCode => rsx! {
                                div {
                                    class: "absolute top-24 z-40 w-full max-w-lg p-3 rounded-lg bg-neutral-900 border border-neutral-700 shadow-xl transition-all duration-300 animate-fade-in",
                                    p {
                                        class: "text-white text-xl text-center font-medium mb-4",
                                        "Please enter the number shown on your other device"
                                    }
                                    div {
                                        class: "flex flex-col justify-center items-center gap-2",

                                        input {
                                            r#type: "text",
                                            inputmode: "numeric",
                                            minlength: "2",
                                            maxlength: "2",
                                            class: "w-3xs px-2 py-1.5 bg-neutral-900 border border-neutral-700 focus-withing:border-blue-500 rounded-md text-white text-center text-sm outline-none transition-all",
                                            value: "{check_code_input}",
                                            oninput: move |e| check_code_input.set(e.value()),
                                        }

                                        button {
                                            class: "w-3xs py-2 bg-blue-600 hover:bg-blue-500 rounded-lg text-white font-medium transition-colors cursor-pointer",
                                            onclick: move |_| {
                                                let input = check_code_input.read().trim().to_owned();

                                                match input.parse::<u8>() {
                                                    Ok(code) if input.len() == 2 => {
                                                        if let Some(tx) = check_code_tx.read().as_ref() {
                                                            tx.send(code).ok();
                                                            check_code_input.set(String::new());
                                                            error.set(None);
                                                        } else {
                                                            error.set(Some("Login flow is no longer active".to_string()));
                                                        }
                                                    }
                                                    _ => {
                                                        error.set(Some("Enter the number shown on your other device".to_string()));
                                                    }
                                                }
                                            },
                                            "Confirm"
                                        }
                                    }
                                }
                            },
                            GeneratedQrState::WaitingForToken(user_code) => rsx! {
                                div {
                                    class: "absolute top-24 z-40 w-full max-w-lg p-3 rounded-lg bg-neutral-900 border border-neutral-700 shadow-xl transition-all duration-300 animate-fade-in",
                                    p {
                                        class: "text-white text-xl text-center font-medium mb-4",
                                        "If asked, enter this code on your other device"
                                    }
                                    p {
                                        class: "flex flex-wrap gap-1 justify-center",
                                        for ch in user_code.chars() {
                                            span {
                                                class: "rounded-md bg-neutral-800 px-4 py-3 text-white text-base font-medium",
                                                "{ch}"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    } else if let Some(state) = qr_state.read().as_ref() {
                        match state {
                            QrState::WaitingForScan => {
                                // TODO: Show a live camera stream
                                rsx! {}
                            },
                            QrState::InputCheckCode(check_code) => rsx! {
                                div {
                                    class: "absolute top-24 z-40 w-full max-w-lg p-3 rounded-lg bg-neutral-900 border border-neutral-700 shadow-xl transition-all duration-300 animate-fade-in",
                                    p {
                                        class: "text-white text-xl text-center font-medium mb-4",
                                        "Please enter this number on your other device"
                                    }
                                    p {
                                        class: "flex flex-wrap gap-1 justify-center",
                                        for ch in check_code.to_string().chars() {
                                            span {
                                                class: "rounded-md bg-neutral-800 px-4 py-3 text-white text-base font-medium",
                                                "{ch}"
                                            }
                                        }
                                    }
                                }
                            },
                            QrState::WaitingForToken(user_code) => rsx! {
                                div {
                                    class: "absolute top-24 z-40 w-full max-w-lg p-3 rounded-lg bg-neutral-900 border border-neutral-700 shadow-xl transition-all duration-300 animate-fade-in",
                                    p {
                                        class: "text-white text-xl text-center font-medium mb-4",
                                        "If asked, enter this code on your other device"
                                    }
                                    p {
                                        class: "flex flex-wrap gap-1 justify-center",
                                        for ch in user_code.chars() {
                                            span {
                                                class: "rounded-md bg-neutral-800 px-4 py-3 text-white text-base font-medium",
                                                "{ch}"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    } else {
                        div {
                            class: "absolute inset-0 z-40 flex flex-col items-center justify-center gap-4",
                            components::Spinner {
                                size: "h-[50px] w-[50px]",
                            }
                        }
                    }
                }

                div {
                    id: "login-window",
                    class: "w-full max-w-85 bg-neutral-800 border border-neutral-700 rounded-xl shadow-2xl transition-[height] duration-300 ease-in-out overflow-hidden",

                    div {
                        id: "login-content",
                        class: "flex flex-col p-4",

                        div {
                            class: "flex flex-col items-center mb-4",
                            components::Icon {
                                size: "h-16 w-16",
                            }
                        }

                        div {
                            id: "login-stage",
                            class: "relative",

                            div {
                                class: format_args!(
                                    "flex flex-col gap-3 transition-opacity duration-200 {}",
                                    if displayed_stage() == LoginStage::Homeserver {
                                        if stage_visible() {
                                            "opacity-100 relative"
                                        } else {
                                            "opacity-0 relative"
                                        }
                                    } else {
                                        "opacity-0 absolute inset-0 pointer-events-none"
                                    }
                                ),

                                TextField {
                                    label: "Homeserver",
                                    value: homeserver,
                                    disabled: is_busy(),
                                    show_error: show_validation_errors() && homeserver.read().is_empty(),
                                }

                                hr {
                                    class: "border-neutral-700 my-2",
                                }

                                if !is_busy() {
                                    button {
                                        class: "w-full py-2 bg-blue-600 hover:bg-blue-500 rounded-lg text-white font-medium transition-colors cursor-pointer",
                                        onclick: move |_| {
                                            error.set(None);
                                            login_choices.set(None);
                                            if homeserver.read().is_empty() {
                                                show_validation_errors.set(true);
                                            } else {
                                                show_validation_errors.set(false);
                                                check_homeserver();
                                            }
                                        },
                                        "Check"
                                    }

                                    if cfg!(any(target_os = "android", target_os = "ios")) {
                                        button {
                                            class: "w-full py-2 bg-neutral-700 hover:bg-neutral-600 rounded-lg text-white text-sm transition-colors cursor-pointer",
                                            onclick: start_oauth_qr_scan_login,
                                            "Continue with QR Code"
                                        }
                                    }
                                } else {
                                    button {
                                        class: "z-60 w-full py-2 bg-red-600 hover:bg-red-500 rounded-lg text-white font-medium transition-colors cursor-pointer",
                                        onclick: move |_| {
                                            cancel_active_task();
                                            is_busy.set(false);
                                            link.set(None);
                                        },
                                        "Cancel"
                                    }
                                }
                            }

                            div {
                                class: format_args!(
                                    "flex flex-col gap-3 transition-opacity duration-200 {}",
                                    if displayed_stage() == LoginStage::Credentials {
                                        if stage_visible() {
                                            "opacity-100 relative"
                                        } else {
                                            "opacity-0 relative"
                                        }
                                    } else {
                                        "opacity-0 absolute inset-0 pointer-events-none"
                                    }
                                ),

                                if let Some(choices) = &*login_choices.read()
                                    && choices.iter().any(|c| matches!(c, auth::LoginChoice::Password))
                                    && choices.iter().all(|c| !matches!(c, auth::LoginChoice::Oauth { preferred: true, .. }))
                                {
                                    TextField {
                                        label: "Username",
                                        value: username,
                                        disabled: is_busy(),
                                        show_error: show_validation_errors() && username.read().is_empty(),
                                    }
                                    TextField {
                                        label: "Password",
                                        value: password,
                                        is_password: true,
                                        disabled: is_busy(),
                                        show_error: show_validation_errors() && password.read().is_empty(),
                                    }

                                    hr {
                                        class: "border-neutral-700 my-1",
                                    }

                                    button {
                                        class: "w-full py-2 bg-blue-600 hover:bg-blue-500 disabled:opacity-50 rounded-lg text-white font-medium transition-colors cursor-pointer",
                                        disabled: is_busy(),
                                        onclick: start_username_login,
                                        "Login"
                                    }
                                }

                                if let Some(choices) = &*login_choices.read()
                                    && choices.iter().any(|c| matches!(c, auth::LoginChoice::Oauth { .. }))
                                {
                                    button {
                                        class: "w-full py-2 bg-neutral-700 hover:bg-neutral-600 rounded-lg text-white text-sm transition-colors cursor-pointer",
                                        onclick: start_oauth_login,
                                        "Continue with Homeserver"
                                    }
                                    if choices.iter().any(|c| matches!(c, auth::LoginChoice::Oauth { supports_msc4108: true, .. }))
                                        && cfg!(not(any(target_os = "android", target_os = "ios")))
                                    {
                                        button {
                                            class: "w-full py-2 bg-neutral-700 hover:bg-neutral-600 rounded-lg text-white text-sm transition-colors cursor-pointer",
                                            onclick: start_oauth_qr_display_login,
                                            "Continue with QR Code"
                                        }
                                    }
                                }

                                if !is_busy() {
                                    button {
                                        class: "w-full py-2 bg-transparent hover:bg-neutral-700/50 rounded-lg text-neutral-400 text-sm transition-colors cursor-pointer",
                                        disabled: is_busy(),
                                        onclick: move |_| {
                                            error.set(None);
                                            show_validation_errors.set(false);
                                            current_stage.set(LoginStage::Homeserver);
                                        },
                                        "Back"
                                    }
                                } else {
                                    button {
                                        class: "z-60 w-full py-2 bg-red-600 hover:bg-red-500 rounded-lg text-white text-sm transition-colors cursor-pointer",
                                        onclick: move |_| {
                                            cancel_active_task();
                                            is_busy.set(false);
                                            link.set(None);
                                        },
                                        "Cancel"
                                    }
                                }
                            }
                        }
                    }
                }
                components::Footer {}
            }
        }
    }
}

#[component]
fn TextField(
    label: &'static str,
    mut value: Signal<String>,
    #[props(default = false)] is_password: bool,
    #[props(default = false)] disabled: bool,
    #[props(default = false)] show_error: bool,
) -> Element {
    let input_type = if is_password { "password" } else { "text" };

    rsx! {
        div {
            class: "flex flex-col gap-1 w-full",

            label {
                class: "text-xs font-semibold text-neutral-300 px-1",
                "{label}"
            }

            div {
                class: "flex items-center w-full bg-neutral-900 border border-neutral-700 focus-within:border-blue-500 rounded-md transition-all disabled:opacity-50",

                input {
                    r#type: "{input_type}",
                    class: "w-full pl-1 pr-3 py-1.5 bg-transparent text-white text-sm outline-none",
                    value: "{value}",
                    disabled,
                    oninput: move |e| value.set(e.value()),
                }
            }

            span {
                class: format_args!("text-xs text-red-400 px-1 animate-fade-in {}",
                    if show_error {
                        "opacity-100"
                    } else {
                        "opacity-0"
                    }
                ),
                "This field is required"
            }
        }
    }
}
