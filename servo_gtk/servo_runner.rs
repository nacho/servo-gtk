/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use async_channel;
use gio::prelude::*;
use gio::{OutputStream, Subprocess, SubprocessFlags, SubprocessLauncher};
use glib::{debug, error, info, warn};
use std::ffi::OsStr;
use std::sync::atomic::{AtomicBool, Ordering};

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
    is_shutdown: AtomicBool,
}

#[allow(clippy::new_without_default)]
impl ServoRunner {
    pub fn new() -> Self {
        info!("Creating new ServoRunner");
        let launcher =
            SubprocessLauncher::new(SubprocessFlags::STDIN_PIPE | SubprocessFlags::STDOUT_PIPE);
        
        info!("Spawning servo-runner subprocess");
        let subprocess = launcher
            .spawn(&[
                OsStr::new("cargo"),
                OsStr::new("run"),
                OsStr::new("--bin"),
                OsStr::new("servo-runner"),
            ])
            .expect("Failed to spawn servo-runner process");

        info!("Servo-runner subprocess spawned successfully");
        let stdin = subprocess.stdin_pipe().expect("Failed to get stdin");
        let stdout = subprocess.stdout_pipe().expect("Failed to get stdout");

        let (event_sender, event_receiver) = async_channel::unbounded();

        info!("Starting IPC event reader task");
        // Async task to receive events from process
        glib::spawn_future_local(glib::clone!(
            #[strong]
            stdout,
            async move {
                debug!("IPC event reader task started");
                let mut message_count = 0u64;
                loop {
                    // Read 4-byte length prefix
                    let len_buf = vec![0u8; 4];
                    match stdout
                        .read_all_future(len_buf, glib::Priority::DEFAULT)
                        .await
                    {
                        Ok((len_buf, bytes_read, _)) => {
                            if bytes_read != 4 {
                                error!("Expected 4 bytes for length prefix, got {}", bytes_read);
                                break;
                            }
                            
                            let len = u32::from_le_bytes([
                                len_buf[0], len_buf[1], len_buf[2], len_buf[3],
                            ]) as usize;
                            
                            debug!("Reading message #{} of {} bytes", message_count + 1, len);

                            if len > 10_000_000 { // 10MB sanity check
                                error!("Message length {} is suspiciously large, breaking", len);
                                break;
                            }

                            // Read message data
                            let msg_buf = vec![0u8; len];
                            match stdout
                                .read_all_future(msg_buf, glib::Priority::DEFAULT)
                                .await
                            {
                                Ok((msg_buf, bytes_read, _)) => {
                                    if bytes_read != len {
                                        error!("Expected {} bytes for message, got {}", len, bytes_read);
                                        break;
                                    }
                                    
                                    match ServoEvent::decode_from_slice(&msg_buf) {
                                        Ok(event) => {
                                            message_count += 1;
                                            debug!("Successfully decoded message #{}", message_count);
                                            if event_sender.send(event).await.is_err() {
                                                warn!("Event receiver dropped, stopping IPC reader");
                                                break;
                                            }
                                        }
                                        Err(e) => {
                                            error!("Failed to decode protobuf message: {:?}", e);
                                            break;
                                        }
                                    }
                                }
                                Err(e) => {
                                    error!("Failed to read message data: {:?}", e);
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to read length prefix: {:?}", e);
                            break;
                        }
                    }
                }
                error!("IPC event reader task ended after {} messages", message_count);
            }
        ));

        info!("ServoRunner created successfully");
        Self {
            stdin,
            event_receiver,
            _subprocess: subprocess,
            is_shutdown: AtomicBool::new(false),
        }
    }

    fn send_action(&self, action: ServoAction) {
        if self.is_shutdown.load(Ordering::Relaxed) {
            warn!("Attempted to send action after shutdown");
            return;
        }
        
        let action_type = action.action.as_ref().map(|a| std::mem::discriminant(a));
        debug!("Sending action: {:?}", action_type);
        
        let stdin = self.stdin.clone();
        glib::spawn_future_local(async move {
            let encoded = action.encode_to_vec();
            let len = (encoded.len() as u32).to_le_bytes();
            
            debug!("Writing action: {} bytes total", encoded.len() + 4);
            
            match stdin.write_all_future(len.to_vec(), glib::Priority::DEFAULT).await {
                Ok(_) => {
                    match stdin.write_all_future(encoded, glib::Priority::DEFAULT).await {
                        Ok(_) => debug!("Action sent successfully"),
                        Err(e) => error!("Failed to write action data: {:?}", e),
                    }
                }
                Err(e) => error!("Failed to write action length: {:?}", e),
            }
        });
    }

    pub fn event_receiver(&self) -> async_channel::Receiver<ServoEvent> {
        self.event_receiver.clone()
    }

    pub fn load_url(&self, url: &str) {
        info!("ServoRunner: Loading URL: {}", url);
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

    pub fn key_press(&self, key: char) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::KeyPress(crate::proto_ipc::KeyPress {
                key: key.to_string(),
            })),
        });
    }

    pub fn key_release(&self, key: char) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::KeyRelease(
                crate::proto_ipc::KeyRelease {
                    key: key.to_string(),
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
        if self.is_shutdown.load(Ordering::Relaxed) {
            warn!("Shutdown called multiple times");
            return;
        }
        
        info!("ServoRunner: Sending shutdown command");
        self.is_shutdown.store(true, Ordering::Relaxed);
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
        info!("ServoRunner being dropped, sending shutdown");
        self.shutdown();
    }
}
