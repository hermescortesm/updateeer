//! Asks the user, with a system notification, whether to update an app that is open.
//! Buttons: "Actualizar ahora" / "Más tarde". Falls back to a dialog when notifications are not
//! available (running outside an .app bundle, or permission denied).
pub enum Reply {
    /// Ids of the apps to update now.
    UpdateNow(Vec<String>),
    Later(Vec<String>),
    /// The notification itself was clicked.
    Open,
}

#[cfg(target_os = "macos")]
mod imp {
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::sync::mpsc::{self, Sender};
    use std::sync::{Arc, Mutex, OnceLock};

    use block2::RcBlock;
    use eframe::egui;
    use objc2::rc::Retained;
    use objc2::runtime::{Bool, ProtocolObject};
    use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
    use objc2_foundation::{NSArray, NSError, NSObject, NSObjectProtocol, NSSet, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotification, UNNotificationAction, UNNotificationActionOptions,
        UNNotificationCategory, UNNotificationCategoryOptions, UNNotificationPresentationOptions, UNNotificationRequest,
        UNNotificationResponse, UNNotificationSound, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
    };

    use super::Reply;
    use crate::model::App;
    use crate::{icons, local, setup};

    const CATEGORY: &str = "update";
    const ACTION_NOW: &str = "now";
    const ACTION_LATER: &str = "later";
    const DEFAULT_ACTION: &str = "com.apple.UNNotificationDefaultActionIdentifier";

    // Authorization result, set from a callback on another thread.
    const UNKNOWN: u8 = 0;
    const GRANTED: u8 = 1;
    const DENIED: u8 = 2;

    struct Sink {
        tx: Sender<Reply>,
        ctx: egui::Context,
    }

    static SINK: OnceLock<Mutex<Sink>> = OnceLock::new();

    fn deliver(reply: Reply) {
        if let Some(sink) = SINK.get().and_then(|m| m.lock().ok()) {
            let _ = sink.tx.send(reply);
            sink.ctx.request_repaint();
        }
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "UpdateeerNotificationDelegate"]
        struct Delegate;

        unsafe impl NSObjectProtocol for Delegate {}

        unsafe impl UNUserNotificationCenterDelegate for Delegate {
            // Show the banner even while Updateeer is the active app.
            #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
            fn will_present(&self, _center: &UNUserNotificationCenter, _n: &UNNotification, done: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>) {
                done.call((UNNotificationPresentationOptions::Banner | UNNotificationPresentationOptions::List | UNNotificationPresentationOptions::Sound,));
            }

            #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
            fn did_receive(&self, _center: &UNUserNotificationCenter, response: &UNNotificationResponse, done: &block2::DynBlock<dyn Fn()>) {
                let ids = split_ids(&response.notification().request().identifier().to_string());
                match response.actionIdentifier().to_string().as_str() {
                    ACTION_NOW => deliver(Reply::UpdateNow(ids)),
                    ACTION_LATER => deliver(Reply::Later(ids)),
                    DEFAULT_ACTION => deliver(Reply::Open),
                    _ => {}
                }
                done.call(());
            }
        }
    );

    pub struct Notifier {
        /// The center only keeps a weak reference to its delegate.
        _delegate: Option<Retained<Delegate>>,
        auth: Arc<AtomicU8>,
        tx: Sender<Reply>,
        ctx: egui::Context,
    }

    /// Notifications need a real app bundle; a bare binary would abort inside UserNotifications.
    fn in_bundle() -> bool {
        std::env::current_exe().is_ok_and(|p| p.to_string_lossy().contains(".app/Contents/MacOS/"))
    }

    impl Notifier {
        pub fn new(ctx: &egui::Context) -> (Self, mpsc::Receiver<Reply>) {
            let (tx, rx) = mpsc::channel();
            let _ = SINK.set(Mutex::new(Sink { tx: tx.clone(), ctx: ctx.clone() }));
            let auth = Arc::new(AtomicU8::new(UNKNOWN));
            let mut delegate = None;
            if let (true, Some(mtm)) = (in_bundle(), MainThreadMarker::new()) {
                let d: Retained<Delegate> = unsafe { msg_send![super(Delegate::alloc(mtm).set_ivars(())), init] };
                let center = UNUserNotificationCenter::currentNotificationCenter();
                center.setDelegate(Some(ProtocolObject::from_ref(&*d)));
                let now = UNNotificationAction::actionWithIdentifier_title_options(&NSString::from_str(ACTION_NOW), &NSString::from_str("Actualizar ahora"), UNNotificationActionOptions::Foreground);
                let later = UNNotificationAction::actionWithIdentifier_title_options(&NSString::from_str(ACTION_LATER), &NSString::from_str("Más tarde"), UNNotificationActionOptions::empty());
                let category = UNNotificationCategory::categoryWithIdentifier_actions_intentIdentifiers_options(
                    &NSString::from_str(CATEGORY),
                    &NSArray::from_retained_slice(&[now, later]),
                    &NSArray::new(),
                    UNNotificationCategoryOptions::empty(),
                );
                center.setNotificationCategories(&NSSet::from_retained_slice(&[category]));
                let flag = auth.clone();
                let done = RcBlock::new(move |granted: Bool, _err: *mut NSError| {
                    flag.store(if granted.as_bool() { GRANTED } else { DENIED }, Ordering::Relaxed);
                });
                center.requestAuthorizationWithOptions_completionHandler(UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound, &done);
                delegate = Some(d);
            }
            (Self { _delegate: delegate, auth, tx, ctx: ctx.clone() }, rx)
        }

        /// Only once the system has said yes: an ad-hoc signed app is refused, and a pending
        /// answer must not swallow the question.
        fn use_notifications(&self) -> bool {
            self._delegate.is_some() && self.auth.load(Ordering::Relaxed) == GRANTED
        }

        /// Asks about open apps (one question for all of them): through a notification when the
        /// system allows it, else a dialog.
        pub fn ask(&self, apps: &[App]) {
            if apps.is_empty() {
                return;
            }
            let (title, body) = super::message(apps);
            let ids = join_ids(apps);
            if self.use_notifications() {
                let content = UNMutableNotificationContent::new();
                content.setTitle(&NSString::from_str(&title));
                content.setBody(&NSString::from_str(&body));
                content.setCategoryIdentifier(&NSString::from_str(CATEGORY));
                content.setSound(Some(&UNNotificationSound::defaultSound()));
                let request = UNNotificationRequest::requestWithIdentifier_content_trigger(&NSString::from_str(&ids), &content, None);
                UNUserNotificationCenter::currentNotificationCenter().addNotificationRequest_withCompletionHandler(&request, None);
            } else {
                dialog(self.tx.clone(), self.ctx.clone(), ids, icon_path(apps), title, body);
            }
        }
    }

    /// Ids travel in the notification identifier; none of them contains a comma.
    fn join_ids(apps: &[App]) -> String {
        apps.iter().map(|a| a.id.as_str()).collect::<Vec<_>>().join(",")
    }

    fn split_ids(s: &str) -> Vec<String> {
        s.split(',').filter(|i| !i.is_empty()).map(String::from).collect()
    }

    /// Icon shown at the left of the dialog: the app's own when asking about one app, else Updateeer's.
    fn icon_path(apps: &[App]) -> Option<std::path::PathBuf> {
        let own = (apps.len() == 1).then(|| local::bundle(&apps[0]).and_then(|b| icons::icns_of(&b))).flatten();
        own.or_else(|| {
            let exe = std::env::current_exe().ok()?;
            Some(exe.parent()?.parent()?.join("Resources/Updateeer.icns")).filter(|p| p.is_file())
        })
    }

    /// Fallback: a two-button dialog from `osascript`, answered on a worker thread.
    fn dialog(tx: Sender<Reply>, ctx: egui::Context, ids: String, icon: Option<std::path::PathBuf>, title: String, body: String) {
        std::thread::spawn(move || {
            let text = setup::applescript_string(&format!("{title}\n{body}"));
            let icon = icon.map(|p| format!(" with icon POSIX file {}", setup::applescript_string(&p.to_string_lossy()))).unwrap_or_default();
            let plural = ids.contains(',');
            let yes = if plural { "Actualizar todas" } else { "Actualizar ahora" };
            let script = format!("display dialog {text} with title \"Updateeer\" buttons {{\"Más tarde\", \"{yes}\"}} default button \"{yes}\"{icon} giving up after 600");
            let out = Command::new("/usr/bin/osascript").args(["-e", &script]).stdin(Stdio::null()).stderr(Stdio::null()).output();
            let reply = match out {
                Ok(o) if String::from_utf8_lossy(&o.stdout).contains(&format!("button returned:{yes}")) => Reply::UpdateNow(split_ids(&ids)),
                _ => Reply::Later(split_ids(&ids)),
            };
            let _ = tx.send(reply);
            ctx.request_repaint();
        });
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use std::sync::mpsc;

    use eframe::egui;

    use super::Reply;
    use crate::model::App;

    /// No system notifications on this OS yet.
    pub struct Notifier;

    impl Notifier {
        pub fn new(_ctx: &egui::Context) -> (Self, mpsc::Receiver<Reply>) {
            (Self, mpsc::channel().1)
        }
        pub fn ask(&self, _apps: &[App]) {}
    }
}

pub use imp::Notifier;

/// Title and body of the question: names every app when there is more than one.
pub fn message(apps: &[crate::model::App]) -> (String, String) {
    match apps {
        [app] => (format!("{} tiene una actualización", app.name), format!("Está abierta ahora. Versión {} → {}.", app.from, app.to)),
        _ => {
            let names: Vec<&str> = apps.iter().take(5).map(|a| a.name.as_str()).collect();
            let list = match (names.split_last(), apps.len() > 5) {
                (Some((last, rest)), false) if !rest.is_empty() => format!("{} y {last}", rest.join(", ")),
                (_, true) => format!("{} y {} más", names.join(", "), apps.len() - 5),
                _ => names.join(", "),
            };
            (format!("{} apps abiertas tienen actualizaciones", apps.len()), format!("{list}: están abiertas ahora."))
        }
    }
}

pub fn supported() -> bool {
    cfg!(target_os = "macos")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{App, Source};

    fn app(name: &str) -> App {
        App { id: format!("cask:{name}"), name: name.into(), vendor: String::new(), from: "1".into(), to: "2".into(), source: Source::Homebrew, size_mb: None, hue: 0.0, bundle: None, installed_at: None }
    }

    #[test]
    fn message_names_the_apps() {
        let (t, b) = message(&[app("Slack")]);
        assert_eq!((t.as_str(), b.as_str()), ("Slack tiene una actualización", "Está abierta ahora. Versión 1 → 2."));
        let (t, b) = message(&[app("A"), app("B"), app("C")]);
        assert_eq!((t.as_str(), b.as_str()), ("3 apps abiertas tienen actualizaciones", "A, B y C: están abiertas ahora."));
        let many: Vec<App> = ["A", "B", "C", "D", "E", "F", "G"].iter().map(|n| app(n)).collect();
        assert_eq!(message(&many).1, "A, B, C, D, E y 2 más: están abiertas ahora.");
    }
}
