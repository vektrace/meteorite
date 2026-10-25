use super::{App, CLIENT, LoadingData, Screen};
use meteorite_core::account::auth;
use slint::ToSharedString;

pub async fn start(weak_app: slint::Weak<App>) {
    match auth::login().await {
        Ok(Some(client)) => {
            *CLIENT.write().await = Some(client);
            weak_app
                .upgrade_in_event_loop(|app| app.set_screen(Screen::Main))
                .ok();
        }
        Ok(None) => {
            weak_app
                .upgrade_in_event_loop(|app| app.set_screen(Screen::Login))
                .ok();
        }
        Err(e) => {
            let msg = e.to_shared_string();
            weak_app
                .upgrade_in_event_loop(|app| app.set_loading_data(LoadingData { error: msg }))
                .ok();
        }
    }
}
