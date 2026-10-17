//! Desktop notifications. On macOS the packaged app uses `UNUserNotificationCenter`, because the older
//! API behind the notification plugin is not delivered on recent macOS. Anything else (dev builds,
//! other platforms) falls back to the plugin.

use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

pub fn show(app: &AppHandle, title: &str, body: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    if mac::packaged() {
        return mac::show(title, body);
    }
    app.notification().builder().title(title).body(body).show().map_err(|error| error.to_string())
}

/// Asks macOS for permission and installs the delegate that lets banners appear while the app is frontmost.
pub fn setup() {
    #[cfg(target_os = "macos")]
    if mac::packaged() {
        mac::setup();
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::{Bool, ProtocolObject};
    use objc2::{define_class, msg_send, AnyThread};
    use objc2_foundation::{NSError, NSObject, NSObjectProtocol, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotification, UNNotificationPresentationOptions, UNNotificationRequest,
        UNNotificationSound, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
    };

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "LocalBoardNotificationDelegate"]
        struct Delegate;

        unsafe impl NSObjectProtocol for Delegate {}

        unsafe impl UNUserNotificationCenterDelegate for Delegate {
            #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
            fn will_present(
                &self,
                _center: &UNUserNotificationCenter,
                _notification: &UNNotification,
                handler: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
            ) {
                handler.call((UNNotificationPresentationOptions::Banner
                    | UNNotificationPresentationOptions::List
                    | UNNotificationPresentationOptions::Sound,));
            }
        }
    );

    /// `UNUserNotificationCenter` raises when the process is not inside an app bundle.
    pub fn packaged() -> bool {
        std::env::current_exe().map(|path| path.to_string_lossy().contains(".app/Contents/MacOS")).unwrap_or(false)
    }

    pub fn setup() {
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let delegate: Retained<Delegate> = unsafe { msg_send![Delegate::alloc(), init] };
        center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        // The centre holds the delegate weakly, so keep it for the life of the process.
        std::mem::forget(delegate);
        let done = RcBlock::new(|granted: Bool, error: *mut NSError| {
            if !granted.as_bool() || !error.is_null() {
                tracing::warn!("notification permission was not granted");
            }
        });
        center.requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound | UNAuthorizationOptions::Badge,
            &done,
        );
    }

    pub fn show(title: &str, body: &str) -> Result<(), String> {
        let center = UNUserNotificationCenter::currentNotificationCenter();
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(title));
        content.setBody(&NSString::from_str(body));
        content.setSound(Some(&UNNotificationSound::defaultSound()));
        let id = NSString::from_str(&uuid::Uuid::new_v4().to_string());
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(&id, &content, None);
        let done = RcBlock::new(|error: *mut NSError| {
            if let Some(error) = unsafe { error.as_ref() } {
                tracing::warn!("could not show a notification: {}", error.localizedDescription());
            }
        });
        center.addNotificationRequest_withCompletionHandler(&request, Some(&done));
        Ok(())
    }
}
