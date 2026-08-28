/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use crate::key_tables::KeyLocation;
use async_channel;
use gio::prelude::*;
use gio::{Subprocess, SubprocessFlags, SubprocessLauncher};
use glib::{debug, error, info, warn};
use std::ffi::OsStr;
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::BorrowedFd;
use std::sync::mpsc;
use std::thread;

use crate::proto_ipc::{ServoAction, ServoEvent, encode_framed, servo_action};

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

/// How many events may sit between the reader thread and the GTK main loop.
///
/// Frame events carry a whole framebuffer, so this is bounded to cap memory if
/// the main loop falls behind. The consumer collapses superseded frames, so in
/// practice the queue holds one.
const EVENT_QUEUE_CAPACITY: usize = 64;

/// Duplicate a gio pipe's file descriptor into an owned [`File`].
///
/// The gio stream objects are not `Send` and so cannot move onto the I/O
/// threads. Duplicating the descriptor gives each thread an independent handle
/// while gio keeps ownership of the original.
fn dup_pipe<T: IsA<glib::Object>>(stream: &T, name: &str) -> File {
    let fd: i32 = stream.property("fd");
    let owned = unsafe { BorrowedFd::borrow_raw(fd) }
        .try_clone_to_owned()
        .unwrap_or_else(|error| panic!("Failed to duplicate servo runner {name}: {error}"));
    File::from(owned)
}

/// Read length-prefixed events off the runner's stdout on a dedicated thread.
///
/// This used to run as a future on the GTK main loop, which meant the UI thread
/// paid for every read, every decode and every multi-megabyte frame copy. Worse,
/// the runner's stdout is a pipe: whenever the UI thread was busy the pipe
/// filled, the runner blocked in `write`, and Servo's event loop stopped
/// entirely. Draining here decouples the two.
fn spawn_event_reader(mut stdout: File, sender: async_channel::Sender<ServoEvent>) {
    thread::spawn(move || {
        // Reused across messages so a steady frame stream does not reallocate.
        let mut msg_buf = Vec::new();
        loop {
            let mut len_buf = [0u8; 4];
            if stdout.read_exact(&mut len_buf).is_err() {
                break;
            }
            let len = u32::from_le_bytes(len_buf) as usize;

            msg_buf.clear();
            msg_buf.resize(len, 0);
            if stdout.read_exact(&mut msg_buf).is_err() {
                break;
            }

            let Ok(event) = ServoEvent::decode_from_slice(&msg_buf) else {
                continue;
            };
            if sender.send_blocking(event).is_err() {
                break;
            }
        }
    });
}

/// Write actions to the runner's stdin on a dedicated thread.
///
/// Sending previously spawned a future per action, each doing two separate
/// async writes; with a motion event per pointer sample that was a steady churn
/// of tasks on the main loop.
fn spawn_action_writer(mut stdin: File) -> mpsc::Sender<ServoAction> {
    let (sender, receiver) = mpsc::channel::<ServoAction>();
    thread::spawn(move || {
        while let Ok(action) = receiver.recv() {
            if stdin.write_all(&encode_framed(&action)).is_err() {
                break;
            }
        }
    });
    sender
}

pub struct ServoRunner {
    action_sender: mpsc::Sender<ServoAction>,
    event_receiver: async_channel::Receiver<ServoEvent>,
    _subprocess: Subprocess,
}

#[allow(clippy::new_without_default)]
impl ServoRunner {
    pub fn new() -> Self {
        let launcher =
            SubprocessLauncher::new(SubprocessFlags::STDIN_PIPE | SubprocessFlags::STDOUT_PIPE);

        // Re-execute this same executable with a marker argument so it starts
        // as the Servo runner subprocess. The consuming application is
        // responsible for calling `servo_gtk::run_as_runner_if_requested()` at
        // the start of `main()` to dispatch to the runner. This removes any
        // need to install a separate binary or invoke `cargo`.
        let current_exe = std::env::current_exe().expect("Failed to get current executable path");
        let subprocess = launcher
            .spawn(&[
                current_exe.as_os_str(),
                OsStr::new(crate::runner::RUNNER_ARG),
            ])
            .expect("Failed to spawn servo runner process");

        let stdin = subprocess.stdin_pipe().expect("Failed to get stdin");
        let stdout = subprocess.stdout_pipe().expect("Failed to get stdout");

        let stdin = dup_pipe(&stdin, "stdin");
        let stdout = dup_pipe(&stdout, "stdout");

        let (event_sender, event_receiver) = async_channel::bounded(EVENT_QUEUE_CAPACITY);
        spawn_event_reader(stdout, event_sender);
        let action_sender = spawn_action_writer(stdin);

        Self {
            action_sender,
            event_receiver,
            _subprocess: subprocess,
        }
    }

    fn send_action(&self, action: ServoAction) {
        let _ = self.action_sender.send(action);
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
