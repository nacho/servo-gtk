/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use crate::key_tables::KeyLocation;
use crate::metrics;
use async_channel;
use gio::prelude::*;
use gio::{Subprocess, SubprocessFlags, SubprocessLauncher};
use glib::{debug, error, info, warn};
use prost::Message;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::ffi::OsStr;
use std::time::{Duration, Instant};

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

/// Maximum number of stamped-but-unanswered input actions remembered.
const MAX_PENDING_INPUTS: usize = 4096;

/// Tracks stamped input actions awaiting a frame that reflects them.
#[derive(Default)]
struct InputTracker {
    next_seq: u64,
    pending: VecDeque<(u64, Instant)>,
}

impl InputTracker {
    /// Allocate the next sequence number (starting at 1) and remember `now`.
    fn stamp(&mut self, now: Instant) -> u64 {
        self.next_seq += 1;
        if self.pending.len() >= MAX_PENDING_INPUTS {
            self.pending.pop_front();
        }
        self.pending.push_back((self.next_seq, now));
        self.next_seq
    }

    /// Pop every entry with seq <= `seq`, returning the elapsed time of each
    /// (oldest first).
    fn complete(&mut self, seq: u64, now: Instant) -> Vec<Duration> {
        let mut done = Vec::new();
        while let Some(&(s, t)) = self.pending.front() {
            if s > seq {
                break;
            }
            self.pending.pop_front();
            done.push(now.saturating_duration_since(t));
        }
        done
    }
}

/// An encoded frame plus, when metrics are on, when it was queued.
type QueuedFrame = (Vec<u8>, Option<Instant>);

/// An event plus the monotonic receive time in microseconds (0 when metrics
/// are disabled).
pub type TimedEvent = (ServoEvent, i64);

pub struct ServoRunner {
    frame_sender: async_channel::Sender<QueuedFrame>,
    event_receiver: async_channel::Receiver<TimedEvent>,
    inputs: RefCell<InputTracker>,
    _subprocess: Subprocess,
}

#[allow(clippy::new_without_default)]
impl ServoRunner {
    pub fn new() -> Self {
        let launcher =
            SubprocessLauncher::new(SubprocessFlags::STDIN_PIPE | SubprocessFlags::STDOUT_PIPE);
        if metrics::enabled() {
            // Enable metrics in the child even when the UI turned them on
            // programmatically rather than through the environment.
            launcher.setenv(metrics::ENV_VAR, "1", true);
        }

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

        let (event_sender, event_receiver) = async_channel::unbounded();

        // Single writer task: frames are drained from this channel and written
        // to the subprocess stdin one at a time, in order. Spawning a separate
        // write future per action (as before) let concurrent writes interleave
        // and corrupt the length-prefixed frame stream, which could silently
        // drop actions such as resize.
        let (frame_sender, frame_receiver) = async_channel::unbounded::<QueuedFrame>();
        glib::spawn_future_local(glib::clone!(
            #[strong]
            stdin,
            async move {
                while let Ok((framed, queued_at)) = frame_receiver.recv().await {
                    if stdin
                        .write_all_future(framed, glib::Priority::DEFAULT)
                        .await
                        .is_err()
                    {
                        break;
                    }
                    metrics::stop("ui.ipc.action_queue_us", queued_at);
                }
            }
        ));

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
                            let read_start = metrics::start();
                            match stdout
                                .read_all_future(msg_buf, glib::Priority::DEFAULT)
                                .await
                            {
                                Ok((msg_buf, _, _)) => {
                                    metrics::stop("ui.ipc.read_body_us", read_start);
                                    let recv_us = if metrics::enabled() {
                                        metrics::monotonic_us()
                                    } else {
                                        0
                                    };
                                    let decode_start = metrics::start();
                                    let decoded = ServoEvent::decode_from_slice(&msg_buf);
                                    metrics::stop("ui.ipc.decode_us", decode_start);
                                    if metrics::enabled() {
                                        metrics::count("ui.ipc.events", 1);
                                        if matches!(
                                            decoded.as_ref().map(|e| &e.event),
                                            Ok(Some(
                                                crate::proto_ipc::servo_event::Event::FrameReady(_)
                                            ))
                                        ) {
                                            metrics::record("ui.ipc.frame_bytes", len as u64);
                                        }
                                    }
                                    if let Ok(event) = decoded
                                        && event_sender.send((event, recv_us)).await.is_err()
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
            frame_sender,
            event_receiver,
            inputs: RefCell::new(InputTracker::default()),
            _subprocess: subprocess,
        }
    }

    fn send_action(&self, mut action: ServoAction) {
        let queued_at = metrics::start();
        if let Some(now) = queued_at {
            metrics::count("ui.ipc.actions", 1);
            if Self::is_input(&action) {
                action.input_seq = self.inputs.borrow_mut().stamp(now);
                metrics::count("ui.inputs", 1);
            }
        }

        // Encode into a length-prefixed frame and hand it to the single writer
        // task, which writes frames sequentially. Ordering is preserved and
        // frames never interleave.
        let len = action.encoded_len();
        let mut framed = Vec::with_capacity(4 + len);
        framed.extend_from_slice(&(len as u32).to_le_bytes());
        action
            .encode(&mut framed)
            .expect("encoding into a Vec cannot fail");

        let sender = self.frame_sender.clone();
        glib::spawn_future_local(async move {
            let _ = sender.send((framed, queued_at)).await;
        });
    }

    fn is_input(action: &ServoAction) -> bool {
        use servo_action::Action::*;
        matches!(
            action.action,
            Some(
                Motion(_)
                    | ButtonPress(_)
                    | ButtonRelease(_)
                    | KeyPress(_)
                    | KeyRelease(_)
                    | Scroll(_)
                    | TouchBegin(_)
                    | TouchUpdate(_)
                    | TouchEnd(_)
                    | TouchCancel(_)
            )
        )
    }

    /// Events from the runner, each with the monotonic receive time in
    /// microseconds (0 when metrics are disabled).
    pub fn event_receiver(&self) -> async_channel::Receiver<TimedEvent> {
        self.event_receiver.clone()
    }

    /// Record `ui.input.to_frame_us` for every stamped input with sequence
    /// <= `seq`, now that a frame reflecting them has arrived.
    pub fn complete_inputs_up_to(&self, seq: u64) {
        if seq == 0 || !metrics::enabled() {
            return;
        }
        let done = self.inputs.borrow_mut().complete(seq, Instant::now());
        if let Some(oldest) = done.first() {
            metrics::record_duration("ui.input.to_frame_oldest_us", *oldest);
        }
        for d in done {
            metrics::record_duration("ui.input.to_frame_us", d);
        }
    }

    /// Ask the runner to send a `MetricsReport`.
    pub fn flush_metrics(&self) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::FlushMetrics(true)),
            ..Default::default()
        });
    }

    /// Clear runner-side metrics.
    pub fn reset_metrics(&self) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::ResetMetrics(true)),
            ..Default::default()
        });
    }

    pub fn load_url(&self, url: &str) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::LoadUrl(crate::proto_ipc::LoadUrl {
                url: url.to_string(),
            })),
            ..Default::default()
        });
    }

    pub fn load_html(&self, html: &str, base_url: Option<&str>) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::LoadHtml(crate::proto_ipc::LoadHtml {
                html: html.to_string(),
                base_url: base_url.unwrap_or_default().to_string(),
            })),
            ..Default::default()
        });
    }

    pub fn reload(&self) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::Reload(true)),
            ..Default::default()
        });
    }

    pub fn go_back(&self) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::GoBack(true)),
            ..Default::default()
        });
    }

    pub fn go_forward(&self) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::GoForward(true)),
            ..Default::default()
        });
    }

    pub fn resize(&self, width: u32, height: u32) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::Resize(crate::proto_ipc::Resize {
                width,
                height,
            })),
            ..Default::default()
        });
    }

    pub fn motion(&self, x: f64, y: f64) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::Motion(crate::proto_ipc::Motion {
                x,
                y,
            })),
            ..Default::default()
        });
    }

    pub fn button_press(&self, button: u32, x: f64, y: f64) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::ButtonPress(
                crate::proto_ipc::ButtonPress { button, x, y },
            )),
            ..Default::default()
        });
    }

    pub fn button_release(&self, button: u32, x: f64, y: f64) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::ButtonRelease(
                crate::proto_ipc::ButtonRelease { button, x, y },
            )),
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
        });
    }

    pub fn scroll(&self, dx: f64, dy: f64) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::Scroll(crate::proto_ipc::Scroll {
                dx,
                dy,
            })),
            ..Default::default()
        });
    }

    pub fn touch_begin(&self, x: f64, y: f64) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::TouchBegin(
                crate::proto_ipc::TouchBegin { x, y },
            )),
            ..Default::default()
        });
    }

    pub fn touch_update(&self, x: f64, y: f64) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::TouchUpdate(
                crate::proto_ipc::TouchUpdate { x, y },
            )),
            ..Default::default()
        });
    }

    pub fn touch_end(&self, x: f64, y: f64) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::TouchEnd(crate::proto_ipc::TouchEnd {
                x,
                y,
            })),
            ..Default::default()
        });
    }

    pub fn touch_cancel(&self, x: f64, y: f64) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::TouchCancel(
                crate::proto_ipc::TouchCancel { x, y },
            )),
            ..Default::default()
        });
    }

    pub fn shutdown(&self) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::Shutdown(true)),
            ..Default::default()
        });
    }

    pub fn add_user_script(&self, source: &str) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::AddUserScript(
                crate::proto_ipc::AddUserScript {
                    source: source.to_string(),
                },
            )),
            ..Default::default()
        });
    }

    pub fn add_user_style_sheet(&self, source: &str) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::AddUserStyleSheet(
                crate::proto_ipc::AddUserStyleSheet {
                    source: source.to_string(),
                },
            )),
            ..Default::default()
        });
    }

    pub fn remove_all_user_scripts(&self) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::RemoveAllUserScripts(true)),
            ..Default::default()
        });
    }

    pub fn remove_all_user_style_sheets(&self) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::RemoveAllUserStyleSheets(true)),
            ..Default::default()
        });
    }

    pub fn register_script_message_handler(&self, name: &str) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::RegisterScriptMessageHandler(
                crate::proto_ipc::RegisterScriptMessageHandler {
                    name: name.to_string(),
                },
            )),
            ..Default::default()
        });
    }

    pub fn unregister_script_message_handler(&self, name: &str) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::UnregisterScriptMessageHandler(
                crate::proto_ipc::UnregisterScriptMessageHandler {
                    name: name.to_string(),
                },
            )),
            ..Default::default()
        });
    }

    pub fn evaluate_javascript(&self, source: &str) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::EvaluateJavascript(
                crate::proto_ipc::EvaluateJavascript {
                    source: source.to_string(),
                },
            )),
            ..Default::default()
        });
    }

    pub fn set_zoom_level(&self, level: f64) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::SetZoomLevel(
                crate::proto_ipc::SetZoomLevel { level },
            )),
            ..Default::default()
        });
    }

    pub fn set_hidpi_scale_factor(&self, scale: f32) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::SetHidpiScaleFactor(
                crate::proto_ipc::SetHidpiScaleFactor { scale },
            )),
            ..Default::default()
        });
    }

    /// Start a request/response round trip that exercises the bidirectional
    /// IPC channel end to end. The runner replies with a `PingRequest` event,
    /// which the [`WebView`](crate::WebView) answers via
    /// [`send_ping_response`](Self::send_ping_response).
    pub fn ping(&self) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::Ping(crate::proto_ipc::Ping {})),
            ..Default::default()
        });
    }

    /// Answer a runner request identified by `request_id` with a ping payload.
    ///
    /// This is the return path of the bidirectional channel: the runner
    /// correlates the response to the pending request it stashed under the same
    /// id. Real delegate features add their own typed `send_*_response`
    /// helpers alongside this one.
    pub fn send_ping_response(&self, request_id: u64, ok: bool) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::Response(
                crate::proto_ipc::ServoResponse {
                    request_id,
                    payload: Some(crate::proto_ipc::servo_response::Payload::Ping(
                        crate::proto_ipc::PingResponse { ok },
                    )),
                },
            )),
            ..Default::default()
        });
    }

    /// Answer a script dialog request (alert/confirm/prompt) identified by
    /// `request_id`. `confirmed` is whether the user accepted; `value` is the
    /// prompt text (ignored for alert/confirm).
    pub fn send_script_dialog_response(&self, request_id: u64, confirmed: bool, value: &str) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::Response(
                crate::proto_ipc::ServoResponse {
                    request_id,
                    payload: Some(crate::proto_ipc::servo_response::Payload::ScriptDialog(
                        crate::proto_ipc::ScriptDialogResponse {
                            confirmed,
                            value: value.to_string(),
                        },
                    )),
                },
            )),
            ..Default::default()
        });
    }

    /// Answer a file chooser request identified by `request_id` with the
    /// selected `paths`. An empty list means the user cancelled.
    pub fn send_file_chooser_response(&self, request_id: u64, paths: Vec<String>) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::Response(
                crate::proto_ipc::ServoResponse {
                    request_id,
                    payload: Some(crate::proto_ipc::servo_response::Payload::FileChooser(
                        crate::proto_ipc::FileChooserResponse { paths },
                    )),
                },
            )),
            ..Default::default()
        });
    }

    /// Answer an HTTP authentication request identified by `request_id`. When
    /// `confirmed` is false the challenge is cancelled and the credentials are
    /// ignored.
    pub fn send_auth_response(
        &self,
        request_id: u64,
        confirmed: bool,
        username: &str,
        password: &str,
    ) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::Response(
                crate::proto_ipc::ServoResponse {
                    request_id,
                    payload: Some(crate::proto_ipc::servo_response::Payload::Auth(
                        crate::proto_ipc::AuthResponse {
                            confirmed,
                            username: username.to_string(),
                            password: password.to_string(),
                        },
                    )),
                },
            )),
            ..Default::default()
        });
    }

    /// Answer a permission request identified by `request_id`.
    pub fn send_permission_response(&self, request_id: u64, allow: bool) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::Response(
                crate::proto_ipc::ServoResponse {
                    request_id,
                    payload: Some(crate::proto_ipc::servo_response::Payload::Permission(
                        crate::proto_ipc::PermissionResponse { allow },
                    )),
                },
            )),
            ..Default::default()
        });
    }

    /// Answer a context menu request identified by `request_id`. When
    /// `selected` is true, `index` is the chosen entry's index; otherwise the
    /// menu was dismissed.
    pub fn send_context_menu_response(&self, request_id: u64, selected: bool, index: u32) {
        self.send_action(ServoAction {
            action: Some(servo_action::Action::Response(
                crate::proto_ipc::ServoResponse {
                    request_id,
                    payload: Some(crate::proto_ipc::servo_response::Payload::ContextMenu(
                        crate::proto_ipc::ContextMenuResponse { selected, index },
                    )),
                },
            )),
            ..Default::default()
        });
    }

    pub fn handle_log_message(&self, level: LogLevel, message: &str) {
        match level {
            LogLevel::Debug => {
                debug!("{}", message);
            }
            LogLevel::Info => {
                info!("{}", message);
            }
            LogLevel::Warn => {
                warn!("{}", message);
            }
            LogLevel::Error => {
                error!("{}", message);
            }
        }
    }
}

impl Drop for ServoRunner {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_tracker_stamps_from_one_and_completes_in_order() {
        let mut t = InputTracker::default();
        let t0 = Instant::now();
        assert_eq!(t.stamp(t0), 1);
        assert_eq!(t.stamp(t0), 2);
        assert_eq!(t.stamp(t0), 3);
        let done = t.complete(2, t0 + Duration::from_millis(5));
        assert_eq!(done.len(), 2);
        assert_eq!(done[0], Duration::from_millis(5));
        assert!(t.complete(2, t0).is_empty());
        assert_eq!(t.complete(10, t0).len(), 1);
    }

    #[test]
    fn input_tracker_caps_pending() {
        let mut t = InputTracker::default();
        let t0 = Instant::now();
        for _ in 0..MAX_PENDING_INPUTS + 10 {
            t.stamp(t0);
        }
        assert_eq!(t.pending.len(), MAX_PENDING_INPUTS);
        assert_eq!(t.pending.front().unwrap().0, 11);
        assert_eq!(t.complete(u64::MAX, t0).len(), MAX_PENDING_INPUTS);
    }
}
