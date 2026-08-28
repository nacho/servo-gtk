/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use glib::info;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, Box, Entry, Orientation, glib};
use servo_gtk::{LoadEvent, WebView};
use std::ptr;

const G_LOG_DOMAIN: &str = "ServoGtkBrowser";

const LOGGER: glib::GlibLogger = glib::GlibLogger::new(
    glib::GlibLoggerFormat::Plain,
    glib::GlibLoggerDomain::CrateTarget,
);

fn main() -> glib::ExitCode {
    // If this process was spawned as the Servo runner subprocess, hand off to
    // it immediately. This never returns when running as the runner.
    servo_gtk::run_as_runner_if_requested();

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

        // Create horizontal box for URL entry and reload button
        let hbox = Box::new(Orientation::Horizontal, 5);

        let url_entry = Entry::builder()
            .placeholder_text("Enter URL...")
            .text("https://example.com")
            .hexpand(true)
            .build();

        let back_button = gtk::Button::from_icon_name("go-previous");
        back_button.set_tooltip_text(Some("Go Back"));

        let forward_button = gtk::Button::from_icon_name("go-next");
        forward_button.set_tooltip_text(Some("Go Forward"));

        let reload_button = gtk::Button::from_icon_name("view-refresh");
        reload_button.set_tooltip_text(Some("Reload"));

        let spinner = gtk::Spinner::new();
        spinner.set_tooltip_text(Some("Loading"));

        let web_view = WebView::new();
        web_view.set_hexpand(true);
        web_view.set_vexpand(true);

        let web_view_clone = web_view.clone();
        url_entry.connect_activate(move |entry| {
            let url = entry.text();
            web_view_clone.load_url(&url);
        });

        let web_view_clone = web_view.clone();
        reload_button.connect_clicked(move |_| {
            web_view_clone.reload();
        });

        let web_view_clone = web_view.clone();
        back_button.connect_clicked(move |_| {
            web_view_clone.go_back();
        });

        let web_view_clone = web_view.clone();
        forward_button.connect_clicked(move |_| {
            web_view_clone.go_forward();
        });

        // Keep the URL entry in sync with the actual page URI via
        // `notify::uri`. This is how a redirect would be observed.
        let url_entry_clone = url_entry.clone();
        web_view.connect_uri_notify(move |web_view| {
            if let Some(uri) = web_view.uri() {
                url_entry_clone.set_text(&uri);
            }
        });

        // Reflect the page title in the window title via `notify::title`.
        let window_clone = window.clone();
        web_view.connect_title_notify(move |web_view| match web_view.title() {
            Some(title) if !title.is_empty() => {
                window_clone.set_title(Some(&format!("{title} — Servo GTK Browser")));
            }
            _ => window_clone.set_title(Some("Servo GTK Browser")),
        });

        // Show a spinner while a load is in progress via `load-changed`.
        let spinner_clone = spinner.clone();
        web_view.connect_load_changed(move |_web_view, event| match event {
            LoadEvent::Started => spinner_clone.start(),
            LoadEvent::Finished => spinner_clone.stop(),
        });

        hbox.append(&back_button);
        hbox.append(&forward_button);
        hbox.append(&reload_button);
        hbox.append(&url_entry);
        hbox.append(&spinner);
        vbox.append(&hbox);
        vbox.append(&web_view);

        window.set_child(Some(&vbox));
        window.present();

        // Load initial URL
        web_view.load_url("https://example.com");
    });

    app.run()
}
