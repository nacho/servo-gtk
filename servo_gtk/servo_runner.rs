/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use crate::key_tables::KeyLocation;
use async_channel;
use gio::prelude::*;
use gio::{OutputStream, Subprocess, SubprocessFlags, SubprocessLauncher};
use glib::{debug, error, info, warn};
use std::ffi::OsStr;

use crate::proto_ipc::{ServoAction, ServoEvent, servo_action};

const G_LOG_DOMAIN: &str = "ServoGtk";

#[derive(Debug, Clone, Copy)]
pub enum LogLevel {
    Debug = 0,
    Info = 1,
    Warn = 2,
    Error = 3,
}

impl From<i32> for LogLevel {
    fn from(value: i32) -> Self {
        match value {
            0 => LogLevel::Debug,
            1 => LogLevel::Info,
            2 => LogLevel::Warn,
            3 => LogLevel::Error,
            _ => LogLevel::Info,
        }
    }
}

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
                loop {
                    // Read 4-byte length prefix
                    let len_buf = vec![0u8; 4];
                    match stdout
                        .read_all_future(len_buf, glib::Priority::DEFAULT)
                        .await
                    {
                        Ok((len_buf, _, _)) => {
                            let len = u32::from_le_bytes([
                                len_buf[0], len_buf[1], len_buf[2], len_buf[3],
                            ]) as usize;

                            // Read message data
                            let msg_buf = vec![0u8; len];
                            match stdout
                                .read_all_future(msg_buf, glib::Priority::DEFAULT)
                                .await
                            {
                                Ok((msg_buf, _, _)) => {
                                    if let Ok(event) = ServoEvent::decode_from_slice(&msg_buf)
                                        && event_sender.send(event).await.is_err()
                                    {
                                        break;
                                    }
                                }
                                Err(_) => break,
                            }
                        }
                        Err(_) => break,
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
            let encoded = action.encode_to_vec();
            let len = (encoded.len() as u32).to_le_bytes();
            let _ = stdin
                .write_all_future(len.to_vec(), glib::Priority::DEFAULT)
                .await;
            let _ = stdin
                .write_all_future(encoded, glib::Priority::DEFAULT)
                .await;
        });
    }

    pub fn event_receiver(&self) -> async_channel::Receiver<ServoEvent> {
        self.event_receiver.clone()
    }

    pub fn load_url(&self, url: &str) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::LoadUrl(crate::proto_ipc::LoadUrl {
                url: url.to_string(),
            })),
        });
    }

    pub fn reload(&self) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::Reload(true)),
        });
    }

    pub fn go_back(&self) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::GoBack(true)),
        });
    }

    pub fn go_forward(&self) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::GoForward(true)),
        });
    }

    pub fn resize(&self, width: u32, height: u32) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::Resize(crate::proto_ipc::Resize {
                width,
                height,
            })),
        });
    }

    pub fn motion(&self, x: f64, y: f64) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::Motion(crate::proto_ipc::Motion {
                x,
                y,
            })),
        });
    }

    pub fn button_press(&self, button: u32, x: f64, y: f64) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::ButtonPress(
                crate::proto_ipc::ButtonPress { button, x, y },
            )),
        });
    }

    pub fn button_release(&self, button: u32, x: f64, y: f64) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::ButtonRelease(
                crate::proto_ipc::ButtonRelease { button, x, y },
            )),
        });
    }

    fn convert_location(location: KeyLocation) -> crate::proto_ipc::Location {
        match location {
            KeyLocation::Standard => crate::proto_ipc::Location::Standard,
            KeyLocation::Left => crate::proto_ipc::Location::Left,
            KeyLocation::Right => crate::proto_ipc::Location::Right,
            KeyLocation::Numpad => crate::proto_ipc::Location::Numpad,
        }
    }

    pub fn key_press(
        &self,
        key: String,
        is_character: bool,
        location: KeyLocation,
        key_code: u32,
        modifiers: u32,
    ) {
        let key_type = if is_character {
            crate::proto_ipc::KeyType::Character
        } else {
            crate::proto_ipc::KeyType::Named
        };
        self.send_action(ServoAction {
            action: Some(servo_action::Action::KeyPress(crate::proto_ipc::KeyPress {
                key,
                key_type: key_type as i32,
                location: Self::convert_location(location) as i32,
                key_code,
                modifiers,
            })),
        });
    }

    pub fn key_release(
        &self,
        key: String,
        is_character: bool,
        location: KeyLocation,
        key_code: u32,
        modifiers: u32,
    ) {
        let key_type = if is_character {
            crate::proto_ipc::KeyType::Character
        } else {
            crate::proto_ipc::KeyType::Named
        };
        self.send_action(ServoAction {
            action: Some(servo_action::Action::KeyRelease(
                crate::proto_ipc::KeyRelease {
                    key,
                    key_type: key_type as i32,
                    location: Self::convert_location(location) as i32,
                    key_code,
                    modifiers,
                },
            )),
        });
    }

    pub fn scroll(&self, dx: f64, dy: f64) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::Scroll(crate::proto_ipc::Scroll {
                dx,
                dy,
            })),
        });
    }

    pub fn touch_begin(&self, x: f64, y: f64) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::TouchBegin(
                crate::proto_ipc::TouchBegin { x, y },
            )),
        });
    }

    pub fn touch_update(&self, x: f64, y: f64) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::TouchUpdate(
                crate::proto_ipc::TouchUpdate { x, y },
            )),
        });
    }

    pub fn touch_end(&self, x: f64, y: f64) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::TouchEnd(crate::proto_ipc::TouchEnd {
                x,
                y,
            })),
        });
    }

    pub fn touch_cancel(&self, x: f64, y: f64) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::TouchCancel(
                crate::proto_ipc::TouchCancel { x, y },
            )),
        });
    }

    pub fn shutdown(&self) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::Shutdown(true)),
        });
    }

    pub fn handle_log_message(&self, level: LogLevel, message: &str) {
        match level {
            LogLevel::Debug => debug!("{}", message),
            LogLevel::Info => info!("{}", message),
            LogLevel::Warn => warn!("{}", message),
            LogLevel::Error => error!("{}", message),
        }
    }
}

impl Drop for ServoRunner {
    fn drop(&mut self) {
        self.shutdown();
    }
}
