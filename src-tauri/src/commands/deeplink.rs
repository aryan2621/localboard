//! `localboard://` deep links.

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_deep_link::DeepLinkExt;

pub const DEEP_LINK_EVENT: &str = "deeplink://open";

/// Forwards `localboard://dashboard/<id>` and `localboard://chat/<id>` to the window.
pub fn register_deep_links(app: &AppHandle) {
    let handle = app.clone();
    app.deep_link().on_open_url(move |event| {
        // Ignored while locked: a link must not surface or navigate the app behind the lock screen.
        if handle.try_state::<crate::state::AppState>().is_none_or(|state| state.ensure_unlocked().is_err()) {
            return;
        }
        for url in event.urls() {
            #[cfg(target_os = "macos")]
            let _ = handle.set_activation_policy(tauri::ActivationPolicy::Regular);
            if let Some(window) = handle.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
            let _ = handle.emit(DEEP_LINK_EVENT, url.to_string());
        }
    });
}
