/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

mod resources;
mod servo_runner;
mod webview;

use glib::info;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, Box, Entry, Orientation, glib};
use std::ptr;
use webview::WebView;

const G_LOG_DOMAIN: &str = "ServoGtk";

const LOGGER: glib::GlibLogger = glib::GlibLogger::new(
    glib::GlibLoggerFormat::Plain,
    glib::GlibLoggerDomain::CrateTarget,
);

fn main() -> glib::ExitCode {
    log::set_logger(&LOGGER).expect("logger already set");
    log::set_max_level(log::LevelFilter::Debug);

    info!("Starting ServoGtk example app");

    let library = unsafe { libloading::os::unix::Library::new("libepoxy.so.0") }.unwrap();
    epoxy::load_with(|name| {
        unsafe { library.get::<_>(name.as_bytes()) }
            .map(|symbol| *symbol)
            .unwrap_or(ptr::null())
    });

    let app = Application::builder()
        .application_id("com.example.ServoGtk")
        .build();

    app.connect_activate(|app| {
        let window = ApplicationWindow::builder()
            .application(app)
            .title("Servo GTK Browser")
            .default_width(1024)
            .default_height(768)
            .build();

        let vbox = Box::new(Orientation::Vertical, 5);

        let url_entry = Entry::builder()
            .placeholder_text("Enter URL...")
            .text("https://example.com")
            .build();

        let webview = WebView::new();
        webview.set_hexpand(true);
        webview.set_vexpand(true);

        let webview_clone = webview.clone();
        url_entry.connect_activate(move |entry| {
            let url = entry.text();
            webview_clone.load_url(&url);
        });

        vbox.append(&url_entry);
        vbox.append(&webview);

        window.set_child(Some(&vbox));
        window.present();

        // Load initial URL
        webview.load_url("https://example.com");
    });

    app.run()
}
