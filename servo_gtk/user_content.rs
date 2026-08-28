/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! User content injection and the page-to-native message channel.
//!
//! This module provides [`UserContentManager`], modelled on WebKitGTK's
//! `WebKitUserContentManager`. It lets an application inject JavaScript
//! ([`UserScript`]) and CSS ([`UserStyleSheet`]) into pages loaded in a
//! [`WebView`](crate::WebView), and register named message handlers that page
//! JavaScript can call to send messages to the native side.
//!
//! # Page-to-native messages
//!
//! After [`UserContentManager::register_script_message_handler`] is called with
//! a name, injected page JavaScript can call:
//!
//! ```js
//! window.servoGtk.messageHandlers.<name>.postMessage(value);
//! ```
//!
//! and the value is delivered to the native side via the
//! [`script-message-received`](UserContentManager::connect_script_message_received)
//! signal, with `body` being the JSON serialization of `value`.
//!
//! # Differences from WebKitGTK
//!
//! Servo (at the revision this library targets) has no native script-message
//! API, so the channel is implemented over Servo's console-message hook: the
//! registered handler is a small injected shim that forwards messages as
//! prefixed console messages, which the runner detects and re-emits. As a
//! consequence:
//!
//! - The JavaScript entry point is `window.servoGtk.messageHandlers`, not
//!   `window.webkit.messageHandlers` (there is no `webkit` alias).
//! - [`UserScript`] carries only a source string; WebKit's injection-time and
//!   injected-frames options are not available in Servo at this revision.
//! - Reply handlers and script worlds are not supported.
//! - User content changes take effect on the next page load.

use glib::prelude::*;
use glib::subclass::Signal;
use gtk::glib;
use gtk::subclass::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::OnceLock;

use crate::servo_runner::ServoRunner;

/// A JavaScript snippet to inject into pages loaded in a
/// [`WebView`](crate::WebView). Analogous to WebKitGTK's `WebKitUserScript`,
/// but carrying only a source string.
#[derive(Debug, Clone)]
pub struct UserScript {
    source: String,
}

impl UserScript {
    /// Create a user script from the given JavaScript source.
    pub fn new(source: &str) -> Self {
        Self {
            source: source.to_string(),
        }
    }

    /// The JavaScript source of this user script.
    pub fn source(&self) -> &str {
        &self.source
    }
}

/// A CSS style sheet to inject into pages loaded in a
/// [`WebView`](crate::WebView). Analogous to WebKitGTK's `WebKitUserStyleSheet`,
/// but carrying only a source string.
#[derive(Debug, Clone)]
pub struct UserStyleSheet {
    source: String,
}

impl UserStyleSheet {
    /// Create a user style sheet from the given CSS source.
    pub fn new(source: &str) -> Self {
        Self {
            source: source.to_string(),
        }
    }

    /// The CSS source of this user style sheet.
    pub fn source(&self) -> &str {
        &self.source
    }
}

/// A buffered user-content action, applied to the [`WebView`]'s runner once the
/// manager is attached to a [`WebView`].
#[derive(Debug, Clone)]
pub(crate) enum PendingAction {
    AddScript(String),
    AddStyleSheet(String),
    RemoveAllScripts,
    RemoveAllStyleSheets,
    RegisterHandler(String),
    UnregisterHandler(String),
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct UserContentManager {
        /// The runner this manager forwards actions to, once attached to a
        /// [`WebView`](crate::WebView). The manager holds only this runner
        /// handle, not the `WebView` itself.
        pub(crate) runner: RefCell<Option<Rc<ServoRunner>>>,
        /// Actions requested before the manager was attached to a runner.
        /// Flushed in order once attached.
        pub(crate) pending: RefCell<Vec<PendingAction>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for UserContentManager {
        const NAME: &'static str = "ServoGtkUserContentManager";
        type Type = super::UserContentManager;
        type ParentType = glib::Object;
    }

    impl ObjectImpl for UserContentManager {
        fn signals() -> &'static [Signal] {
            static SIGNALS: OnceLock<Vec<Signal>> = OnceLock::new();
            SIGNALS.get_or_init(|| {
                vec![
                    // Emitted when page JavaScript sends a message through a
                    // registered handler. Parameters: handler name, JSON body.
                    Signal::builder("script-message-received")
                        .param_types([String::static_type(), String::static_type()])
                        .build(),
                ]
            })
        }
    }
}

glib::wrapper! {
    pub struct UserContentManager(ObjectSubclass<imp::UserContentManager>);
}

impl Default for UserContentManager {
    fn default() -> Self {
        Self::new()
    }
}

impl UserContentManager {
    /// Create a new, empty user content manager.
    pub fn new() -> Self {
        glib::Object::builder().build()
    }

    /// Inject a [`UserScript`] into pages loaded in the associated
    /// [`WebView`]. Takes effect on the next page load.
    pub fn add_script(&self, script: &UserScript) {
        self.dispatch(PendingAction::AddScript(script.source().to_string()));
    }

    /// Inject a [`UserStyleSheet`] into pages loaded in the associated
    /// [`WebView`]. Takes effect on the next page load.
    pub fn add_style_sheet(&self, style_sheet: &UserStyleSheet) {
        self.dispatch(PendingAction::AddStyleSheet(
            style_sheet.source().to_string(),
        ));
    }

    /// Remove all user scripts previously added to this manager.
    pub fn remove_all_scripts(&self) {
        self.dispatch(PendingAction::RemoveAllScripts);
    }

    /// Remove all user style sheets previously added to this manager.
    pub fn remove_all_style_sheets(&self) {
        self.dispatch(PendingAction::RemoveAllStyleSheets);
    }

    /// Register a named script message handler. After this, page JavaScript
    /// can call `window.servoGtk.messageHandlers.<name>.postMessage(value)` to
    /// deliver a message to the native side via the
    /// [`script-message-received`](Self::connect_script_message_received)
    /// signal. Takes effect on the next page load.
    pub fn register_script_message_handler(&self, name: &str) {
        self.dispatch(PendingAction::RegisterHandler(name.to_string()));
    }

    /// Unregister a previously registered named script message handler.
    ///
    /// Note: Servo cannot un-inject an already-applied handler shim, so this
    /// only prevents future injection; existing pages keep the handler until
    /// reloaded.
    pub fn unregister_script_message_handler(&self, name: &str) {
        self.dispatch(PendingAction::UnregisterHandler(name.to_string()));
    }

    /// Connect to the `script-message-received` signal, emitted when page
    /// JavaScript posts a message through a registered handler. The closure
    /// receives the handler `name` and the JSON-serialized message `body`.
    pub fn connect_script_message_received<F: Fn(&Self, &str, &str) + 'static>(
        &self,
        f: F,
    ) -> glib::SignalHandlerId {
        self.connect_closure(
            "script-message-received",
            false,
            glib::closure_local!(move |obj: &Self, name: String, body: String| {
                f(obj, &name, &body);
            }),
        )
    }

    /// Attach this manager to a [`WebView`](crate::WebView)'s runner, flushing
    /// any buffered actions. Called by `WebView` construction; not part of the
    /// public API.
    pub(crate) fn attach(&self, runner: Rc<ServoRunner>) {
        self.imp().runner.replace(Some(runner));
        // Flush buffered actions in order.
        let pending: Vec<PendingAction> = self.imp().pending.borrow_mut().drain(..).collect();
        for action in pending {
            self.apply(&action);
        }
    }

    /// Emit the `script-message-received` signal. Called by
    /// [`WebView`](crate::WebView) when a ScriptMessage event arrives from the
    /// runner.
    pub(crate) fn emit_script_message(&self, name: &str, body: &str) {
        self.emit_by_name::<()>("script-message-received", &[&name, &body]);
    }

    /// Forward an action to the attached runner, or buffer it if not yet
    /// attached.
    fn dispatch(&self, action: PendingAction) {
        if self.imp().runner.borrow().is_some() {
            self.apply(&action);
        } else {
            self.imp().pending.borrow_mut().push(action);
        }
    }

    fn apply(&self, action: &PendingAction) {
        let runner = self.imp().runner.borrow();
        let Some(runner) = runner.as_ref() else {
            return;
        };
        match action {
            PendingAction::AddScript(source) => runner.add_user_script(source),
            PendingAction::AddStyleSheet(source) => runner.add_user_style_sheet(source),
            PendingAction::RemoveAllScripts => runner.remove_all_user_scripts(),
            PendingAction::RemoveAllStyleSheets => runner.remove_all_user_style_sheets(),
            PendingAction::RegisterHandler(name) => runner.register_script_message_handler(name),
            PendingAction::UnregisterHandler(name) => {
                runner.unregister_script_message_handler(name)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_script_and_style_sheet_carry_source() {
        let script = UserScript::new("console.log('hi')");
        assert_eq!(script.source(), "console.log('hi')");
        let sheet = UserStyleSheet::new("body { color: red }");
        assert_eq!(sheet.source(), "body { color: red }");
    }

    #[test]
    fn manager_registers_signal() {
        let ucm = UserContentManager::new();
        // Connecting to the signal must succeed (signal is registered).
        let _id = ucm.connect_script_message_received(|_, _, _| {});
    }

    #[test]
    fn actions_are_buffered_before_attach() {
        let ucm = UserContentManager::new();
        ucm.add_script(&UserScript::new("1"));
        ucm.add_style_sheet(&UserStyleSheet::new("2"));
        ucm.register_script_message_handler("h");
        // Not attached to a runner yet, so all actions should be buffered.
        assert_eq!(ucm.imp().pending.borrow().len(), 3);
    }

    #[test]
    fn script_message_signal_delivers_name_and_body() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let ucm = UserContentManager::new();
        let received = Rc::new(RefCell::new(None));
        let received_clone = received.clone();
        ucm.connect_script_message_received(move |_, name, body| {
            *received_clone.borrow_mut() = Some((name.to_string(), body.to_string()));
        });

        ucm.emit_script_message("auth", r#"{"token":"abc"}"#);

        assert_eq!(
            *received.borrow(),
            Some(("auth".to_string(), r#"{"token":"abc"}"#.to_string()))
        );
    }
}
