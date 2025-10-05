/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use glib::info;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, Box, Entry, Orientation, glib};
use servo_gtk::WebView;
use std::ptr;

const G_LOG_DOMAIN: &str = "ServoGtkBrowser";

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

        hbox.append(&back_button);
        hbox.append(&forward_button);
        hbox.append(&reload_button);
        hbox.append(&url_entry);
        vbox.append(&hbox);
        vbox.append(&web_view);

        window.set_child(Some(&vbox));
        window.present();

        // Load initial URL
        web_view.load_url("https://example.com");
    });

    app.run()
}
