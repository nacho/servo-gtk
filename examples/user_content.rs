/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Example: user-script injection and the page-to-native message channel.
//!
//! This demonstrates the WebKit-style user-content API added to servo-gtk:
//!
//! 1. Create a [`UserContentManager`] and construct a [`WebView`] with it via
//!    [`WebView::with_user_content_manager`].
//! 2. Register a named message handler and inject a user script that posts a
//!    message to it — mirroring how a JavaScript SPA talks to its native host.
//! 3. Receive the message on the native side via the
//!    `script-message-received` signal and display it.
//! 4. Trigger `evaluate_javascript` from a button to push a message from
//!    native to the page (which then posts back).
//!
//! The page-to-native call in JavaScript looks like:
//!
//! ```js
//! window.servoGtk.messageHandlers.demo.postMessage({ hello: "native" });
//! ```
//!
//! Run with: `cargo run --example user_content` (requires a display).

use glib::info;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, Box, Button, Label, Orientation, glib};
use servo_gtk::{UserContentManager, UserScript, WebView};
use std::ptr;

const G_LOG_DOMAIN: &str = "ServoGtkUserContent";

const LOGGER: glib::GlibLogger = glib::GlibLogger::new(
    glib::GlibLoggerFormat::Plain,
    glib::GlibLoggerDomain::CrateTarget,
);

/// Name of the message handler the page uses to talk to native.
const HANDLER_NAME: &str = "demo";

fn main() -> glib::ExitCode {
    // If this process was spawned as the Servo runner subprocess, hand off to
    // it immediately. This never returns when running as the runner.
    servo_gtk::run_as_runner_if_requested();

    log::set_logger(&LOGGER).expect("logger already set");
    log::set_max_level(log::LevelFilter::Debug);

    info!("Starting ServoGtk user-content example");

    let library = unsafe { libloading::os::unix::Library::new("libepoxy.so.0") }.unwrap();
    epoxy::load_with(|name| {
        unsafe { library.get::<_>(name.as_bytes()) }
            .map(|symbol| *symbol)
            .unwrap_or(ptr::null())
    });

    let app = Application::builder()
        .application_id("com.example.ServoGtkUserContent")
        .build();

    app.connect_activate(|app| {
        let window = ApplicationWindow::builder()
            .application(app)
            .title("Servo GTK — User Content")
            .default_width(1024)
            .default_height(768)
            .build();

        let vbox = Box::new(Orientation::Vertical, 5);

        // A label showing the most recent message received from the page.
        let status = Label::new(Some("Waiting for a message from the page…"));
        status.set_wrap(true);
        status.set_xalign(0.0);

        // 1. Create the user content manager.
        let ucm = UserContentManager::new();

        // 2. Register a named handler so page JS can post to it. Injected JS
        //    can then call:
        //      window.servoGtk.messageHandlers.demo.postMessage(value)
        ucm.register_script_message_handler(HANDLER_NAME);

        // Inject a user script that posts a greeting once the page has loaded,
        // and exposes a helper the "Ping page" button below can invoke.
        let user_script = UserScript::new(
            r#"
            (function () {
              function post(payload) {
                window.servoGtk.messageHandlers.demo.postMessage(payload);
              }
              // Post an initial message after the document is ready.
              document.addEventListener('DOMContentLoaded', function () {
                post({ type: 'HELLO', title: document.title, url: location.href });
              });
              // Expose a function native code can call via evaluate_javascript.
              window.demoPing = function (note) {
                post({ type: 'PONG', note: note });
              };
            })();
            "#,
        );
        ucm.add_script(&user_script);

        // 3. Receive messages from the page on the native side.
        let status_clone = status.clone();
        ucm.connect_script_message_received(move |_ucm, name, body| {
            info!("script message from '{name}': {body}");
            status_clone.set_text(&format!("Received from '{name}': {body}"));
        });

        // Construct the WebView with the manager attached.
        let web_view = WebView::with_user_content_manager(&ucm);
        web_view.set_hexpand(true);
        web_view.set_vexpand(true);

        // 4. A button that pushes a message from native to the page by
        //    evaluating JavaScript; the page's window.demoPing posts back.
        let ping_button = Button::with_label("Ping page (native → page → native)");
        let web_view_clone = web_view.clone();
        ping_button.connect_clicked(move |_| {
            web_view_clone
                .evaluate_javascript("window.demoPing && window.demoPing('from native');");
        });

        let hbox = Box::new(Orientation::Horizontal, 5);
        hbox.append(&ping_button);
        hbox.append(&status);

        vbox.append(&hbox);
        vbox.append(&web_view);

        window.set_child(Some(&vbox));
        window.present();

        // Load a page. The injected script runs on load and posts the initial
        // HELLO message back to native.
        web_view.load_url("https://example.com");
    });

    app.run()
}
