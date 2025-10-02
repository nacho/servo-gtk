/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use async_channel;
use gio::prelude::*;
use gio::{DataInputStream, OutputStream, Subprocess, SubprocessFlags, SubprocessLauncher};
use std::ffi::OsStr;

use crate::ipc::{ServoAction, ServoEvent};

pub struct ServoRunner {
    stdin: OutputStream,
    event_receiver: async_channel::Receiver<ServoEvent>,
    _subprocess: Subprocess,
}

#[allow(clippy::new_without_default)]
impl ServoRunner {
    pub fn new() -> Self {
        let launcher =
            SubprocessLauncher::new(SubprocessFlags::STDIN_PIPE | SubprocessFlags::STDOUT_PIPE);
        let subprocess = launcher
            .spawn(&[
                OsStr::new("cargo"),
                OsStr::new("run"),
                OsStr::new("--bin"),
                OsStr::new("servo-runner"),
            ])
            .expect("Failed to spawn servo-runner process");

        let stdin = subprocess.stdin_pipe().expect("Failed to get stdin");
        let stdout = subprocess.stdout_pipe().expect("Failed to get stdout");

        let (event_sender, event_receiver) = async_channel::unbounded();

        // Async task to receive events from process
        glib::spawn_future_local(glib::clone!(
            #[strong]
            stdout,
            async move {
                let data_stream = DataInputStream::new(&stdout);
                while let Ok(Some(line)) =
                    data_stream.read_line_future(glib::Priority::DEFAULT).await
                {
                    let line_str = String::from_utf8_lossy(&line);
                    if let Ok(event) = serde_json::from_str::<ServoEvent>(&line_str)
                        && event_sender.send(event).await.is_err()
                    {
                        break;
                    }
                }
            }
        ));

        Self {
            stdin,
            event_receiver,
            _subprocess: subprocess,
        }
    }

    fn send_action(&self, action: ServoAction) {
        let stdin = self.stdin.clone();
        glib::spawn_future_local(async move {
            if let Ok(json) = serde_json::to_string(&action) {
                let data = format!("{}\n", json);
                let bytes = data.into_bytes();
                let _ = stdin.write_all_future(bytes, glib::Priority::DEFAULT).await;
            }
        });
    }

    pub fn event_receiver(&self) -> async_channel::Receiver<ServoEvent> {
        self.event_receiver.clone()
    }

    pub fn load_url(&self, url: &str) {
        self.send_action(ServoAction::LoadUrl(url.to_string()));
    }

    pub fn reload(&self) {
        self.send_action(ServoAction::Reload);
    }

    pub fn go_back(&self) {
        self.send_action(ServoAction::GoBack);
    }

    pub fn go_forward(&self) {
        self.send_action(ServoAction::GoForward);
    }

    pub fn resize(&self, width: u32, height: u32) {
        self.send_action(ServoAction::Resize(width, height));
    }

    pub fn motion(&self, x: f64, y: f64) {
        self.send_action(ServoAction::Motion(x, y));
    }

    pub fn button_press(&self, button: u32, x: f64, y: f64) {
        self.send_action(ServoAction::ButtonPress(button, x, y));
    }

    pub fn button_release(&self, button: u32, x: f64, y: f64) {
        self.send_action(ServoAction::ButtonRelease(button, x, y));
    }

    pub fn key_press(&self, key: char) {
        self.send_action(ServoAction::KeyPress(key));
    }

    pub fn key_release(&self, key: char) {
        self.send_action(ServoAction::KeyRelease(key));
    }

    pub fn scroll(&self, dx: f64, dy: f64) {
        self.send_action(ServoAction::Scroll(dx, dy));
    }

    pub fn touch_begin(&self, x: f64, y: f64) {
        self.send_action(ServoAction::TouchBegin(x, y));
    }

    pub fn touch_update(&self, x: f64, y: f64) {
        self.send_action(ServoAction::TouchUpdate(x, y));
    }

    pub fn touch_end(&self, x: f64, y: f64) {
        self.send_action(ServoAction::TouchEnd(x, y));
    }

    pub fn touch_cancel(&self, x: f64, y: f64) {
        self.send_action(ServoAction::TouchCancel(x, y));
    }

    pub fn shutdown(&self) {
        self.send_action(ServoAction::Shutdown);
    }
}

impl Drop for ServoRunner {
    fn drop(&mut self) {
        self.shutdown();
    }
}
