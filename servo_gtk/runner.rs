/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Servo runner subprocess entry point.
//!
//! The runner is not a separately installed binary. Instead, the library
//! re-executes the consuming application's own executable with a marker
//! argument (see [`RUNNER_ARG`]). The consumer is responsible for calling
//! [`run_if_requested`] at the very start of its `main()` so that, when the
//! process was spawned as a runner, control is handed off here and never
//! returns to the normal application startup path.

use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::BorrowedFd;
use std::rc::Rc;
use std::sync::{Mutex, OnceLock};

use core::time::Duration;
use dpi::PhysicalSize;
use embedder_traits::{WebViewPoint, WebViewVector};
use euclid::Point2D;
use keyboard_types::{Code, Key, KeyState, Location, Modifiers, NamedKey};

use servo::{
    DeviceIntRect, DeviceVector2D, InputEvent, KeyboardEvent, MouseButton, MouseButtonAction,
    MouseButtonEvent, MouseMoveEvent, Scroll, ServoBuilder,
};
use servo::{RenderingContext, SoftwareRenderingContext, WebView, WebViewBuilder, WebViewDelegate};
use std::str::FromStr;
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use std::thread;
use url::Url;

use crate::proto_ipc::{
    CursorChanged, FrameReady, LogLevel, LogMessage, ServoAction, ServoEvent, encode_framed,
    servo_action, servo_event,
};

/// Marker argument used to signal that the process should run as a Servo
/// runner subprocess rather than as the host application.
pub const RUNNER_ARG: &str = "--servo-gtk-runner";

/// If the current process was spawned as a Servo runner (i.e. its arguments
/// contain [`RUNNER_ARG`]), run the runner loop and terminate the process,
/// never returning. Otherwise, return immediately so normal application
/// startup can proceed.
///
/// Consumers MUST call this as the very first thing in `main()`:
///
/// ```ignore
/// fn main() {
///     servo_gtk::run_as_runner_if_requested();
///     // ... normal application startup ...
/// }
/// ```
pub fn run_if_requested() {
    // Only consider arguments before a `--` separator: by convention anything
    // after `--` is positional/escaped and must not be interpreted as an
    // option. The library always passes `RUNNER_ARG` as a regular argument
    // when spawning, so it will always appear before any `--`.
    let is_runner = std::env::args()
        .skip(1)
        .take_while(|arg| arg != "--")
        .any(|arg| arg == RUNNER_ARG);
    if is_runner {
        run();
        std::process::exit(0);
    }
}

struct EventLogger {
    sender: std::sync::mpsc::Sender<LogMessage>,
}

impl EventLogger {
    fn new() -> (Self, std::sync::mpsc::Receiver<LogMessage>) {
        let (sender, receiver) = std::sync::mpsc::channel();
        (Self { sender }, receiver)
    }
}

impl log::Log for EventLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }

        let level = match record.level() {
            log::Level::Error => LogLevel::Error,
            log::Level::Warn => LogLevel::Warn,
            log::Level::Info => LogLevel::Info,
            log::Level::Debug => LogLevel::Debug,
            log::Level::Trace => LogLevel::Debug,
        };

        let log_message = LogMessage {
            level: level as i32,
            message: format!("{}", record.args()),
        };

        let _ = self.sender.send(log_message);
    }

    fn flush(&self) {}
}

/// Maximum log level for the runner, overridable with `SERVO_GTK_LOG`.
///
/// This defaults to `Warn` deliberately. Every record produced here is
/// formatted into a `String`, encoded, pushed through the IPC pipe and
/// re-logged on the host's GTK main thread, so a permissive level is not a
/// passive cost: at `Debug`, Servo's style system emits a record per selector
/// match per element and the host's UI thread saturates handling them.
fn max_log_level() -> log::LevelFilter {
    match std::env::var("SERVO_GTK_LOG") {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "off" => log::LevelFilter::Off,
            "error" => log::LevelFilter::Error,
            "warn" => log::LevelFilter::Warn,
            "info" => log::LevelFilter::Info,
            "debug" => log::LevelFilter::Debug,
            "trace" => log::LevelFilter::Trace,
            _ => log::LevelFilter::Warn,
        },
        Err(_) => log::LevelFilter::Warn,
    }
}

/// The write end of the IPC pipe.
///
/// `io::stdout()` is a `LineWriter`, which scans every buffer it is given for a
/// newline; on a multi-megabyte frame that is a pointless pass over the whole
/// payload, and it leaves the bytes after the last newline sitting in the
/// buffer until some later write flushes them. A plain `File` on a dup of fd 1
/// writes each message straight through instead.
fn event_pipe() -> &'static Mutex<File> {
    static PIPE: OnceLock<Mutex<File>> = OnceLock::new();
    PIPE.get_or_init(|| {
        let stdout = unsafe { BorrowedFd::borrow_raw(1) };
        let owned = stdout
            .try_clone_to_owned()
            .expect("Failed to duplicate stdout");
        Mutex::new(File::from(owned))
    })
}

fn send_event(event: ServoEvent) -> std::io::Result<()> {
    let framed = encode_framed(&event);
    event_pipe()
        .lock()
        .expect("Event pipe mutex poisoned")
        .write_all(&framed)
}

struct ServoWebViewDelegate {
    rendering_context: Rc<dyn RenderingContext>,
}

impl ServoWebViewDelegate {
    fn new(rendering_context: Rc<dyn RenderingContext>) -> Self {
        Self { rendering_context }
    }
}

impl WebViewDelegate for ServoWebViewDelegate {
    fn notify_new_frame_ready(&self, webview: WebView) {
        let size = self.rendering_context.size2d().to_i32();
        let viewport_rect = DeviceIntRect::from_origin_and_size(Point2D::origin(), size);
        webview.paint();
        self.rendering_context.present();

        if let Some(rgba_image) = self.rendering_context.read_to_image(viewport_rect) {
            let width = rgba_image.width();
            let height = rgba_image.height();
            let data = rgba_image.into_raw();

            let event = ServoEvent {
                event: Some(servo_event::Event::FrameReady(FrameReady {
                    rgba_data: data,
                    width,
                    height,
                })),
            };
            let _ = send_event(event);
        }
    }

    fn notify_cursor_changed(&self, _webview: servo::WebView, cursor: servo::Cursor) {
        let cursor_str = match cursor {
            servo::Cursor::Default => "default",
            servo::Cursor::Pointer => "pointer",
            servo::Cursor::Text => "text",
            servo::Cursor::Wait => "wait",
            servo::Cursor::Help => "help",
            servo::Cursor::Crosshair => "crosshair",
            servo::Cursor::Move => "move",
            servo::Cursor::EResize => "e-resize",
            servo::Cursor::NeResize => "ne-resize",
            servo::Cursor::NwResize => "nw-resize",
            servo::Cursor::NResize => "n-resize",
            servo::Cursor::SeResize => "se-resize",
            servo::Cursor::SwResize => "sw-resize",
            servo::Cursor::SResize => "s-resize",
            servo::Cursor::WResize => "w-resize",
            servo::Cursor::EwResize => "ew-resize",
            servo::Cursor::NsResize => "ns-resize",
            servo::Cursor::NeswResize => "nesw-resize",
            servo::Cursor::NwseResize => "nwse-resize",
            servo::Cursor::ColResize => "col-resize",
            servo::Cursor::RowResize => "row-resize",
            servo::Cursor::AllScroll => "all-scroll",
            servo::Cursor::ZoomIn => "zoom-in",
            servo::Cursor::ZoomOut => "zoom-out",
            servo::Cursor::Alias => "alias",
            servo::Cursor::Cell => "cell",
            servo::Cursor::Copy => "copy",
            servo::Cursor::ContextMenu => "context-menu",
            servo::Cursor::NoDrop => "no-drop",
            servo::Cursor::NotAllowed => "not-allowed",
            servo::Cursor::Grab => "grab",
            servo::Cursor::Grabbing => "grabbing",
            servo::Cursor::VerticalText => "vertical-text",
            servo::Cursor::Progress => "progress",
            _ => "default",
        };
        let event = ServoEvent {
            event: Some(servo_event::Event::CursorChanged(CursorChanged {
                cursor: cursor_str.to_string(),
            })),
        };
        let _ = send_event(event);
    }
}

fn init_crypto() {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("Error initializing crypto provider");
}

fn spawn_stdin_channel() -> Receiver<ServoAction> {
    let (tx, rx) = mpsc::channel::<ServoAction>();
    thread::spawn(move || {
        let mut stdin = io::stdin();
        loop {
            let mut len_buf = [0u8; 4];
            if stdin.read_exact(&mut len_buf).is_err() {
                break;
            }
            let len = u32::from_le_bytes(len_buf) as usize;

            let mut msg_buf = vec![0u8; len];
            if stdin.read_exact(&mut msg_buf).is_err() {
                break;
            }

            if let Ok(action) = ServoAction::decode_from_slice(&msg_buf)
                && tx.send(action).is_err()
            {
                break;
            }
        }
    });
    rx
}

fn convert_location(proto_location: crate::proto_ipc::Location) -> Location {
    match proto_location {
        crate::proto_ipc::Location::Standard => Location::Standard,
        crate::proto_ipc::Location::Left => Location::Left,
        crate::proto_ipc::Location::Right => Location::Right,
        crate::proto_ipc::Location::Numpad => Location::Numpad,
    }
}

fn convert_key_event(
    key_str: String,
    key_type: i32,
    location: i32,
    key_code: u32,
    modifiers: u32,
    state: KeyState,
) -> KeyboardEvent {
    let key = match crate::proto_ipc::KeyType::try_from(key_type)
        .unwrap_or(crate::proto_ipc::KeyType::Character)
    {
        crate::proto_ipc::KeyType::Character => Key::Character(key_str),
        crate::proto_ipc::KeyType::Named => {
            Key::Named(NamedKey::from_str(&key_str).unwrap_or(NamedKey::Unidentified))
        }
    };
    let location = convert_location(
        crate::proto_ipc::Location::try_from(location)
            .unwrap_or(crate::proto_ipc::Location::Standard),
    );
    let modifiers = Modifiers::from_bits_truncate(modifiers);
    // TODO: Convert key_code to proper Code enum value
    let _code = key_code; // Keep for future use
    let code = Code::Unidentified;
    KeyboardEvent::new_without_event(state, key, code, location, modifiers, false, false)
}

/// How long an otherwise idle iteration waits for input before spinning the
/// Servo event loop again.
const SPIN_INTERVAL: Duration = Duration::from_millis(5);

/// Upper bound on log messages forwarded per iteration, so that a chatty log
/// level cannot starve input handling or the Servo event loop.
const MAX_LOG_MESSAGES_PER_ITERATION: usize = 256;

/// Queue `action` for processing, merging it into the previous one when the
/// intermediate states carry no information.
///
/// Pointer motion is absolute, so only the newest position matters, and scroll
/// deltas are additive, so a burst applies as one larger delta. Only runs of
/// the same kind merge, which keeps every event ordered against the clicks and
/// key presses around it.
fn push_coalesced(pending: &mut Vec<servo_action::Action>, action: ServoAction) {
    let Some(action) = action.action else {
        return;
    };

    match (pending.last_mut(), &action) {
        (Some(servo_action::Action::Motion(last)), servo_action::Action::Motion(new)) => {
            last.x = new.x;
            last.y = new.y;
            return;
        }
        (Some(servo_action::Action::Scroll(last)), servo_action::Action::Scroll(new)) => {
            last.dx += new.dx;
            last.dy += new.dy;
            return;
        }
        _ => {}
    }

    pending.push(action);
}

/// Run the Servo runner event loop. This blocks until a shutdown action is
/// received or the IPC pipes are closed.
pub fn run() {
    let (event_logger, log_receiver) = EventLogger::new();

    log::set_logger(Box::leak(Box::new(event_logger))).expect("Failed to set logger");
    log::set_max_level(max_log_level());

    init_crypto();

    log::info!("Starting servo runner");

    let size = PhysicalSize::new(800, 600);
    let rendering_context = Rc::new(
        SoftwareRenderingContext::new(size).expect("Failed to create Software rendering context"),
    );

    let servo_builder = ServoBuilder::default();
    let servo = servo_builder.build();

    let delegate = Rc::new(ServoWebViewDelegate::new(rendering_context.clone()));
    let webview = WebViewBuilder::new(&servo, rendering_context)
        .delegate(delegate)
        .build();

    let receiver = spawn_stdin_channel();

    loop {
        // Process queued log messages
        for _ in 0..MAX_LOG_MESSAGES_PER_ITERATION {
            let Ok(log_message) = log_receiver.try_recv() else {
                break;
            };
            let event = ServoEvent {
                event: Some(servo_event::Event::LogMessage(log_message)),
            };
            let _ = send_event(event);
        }

        // Block briefly for input, then drain everything else already queued.
        // Handling one action per iteration and sleeping in between capped
        // input at 200 actions/second, so a burst of motion or scroll events
        // took hundreds of milliseconds to work through and lagged visibly
        // behind the pointer.
        let mut pending = Vec::new();
        match receiver.recv_timeout(SPIN_INTERVAL) {
            Ok(action) => push_coalesced(&mut pending, action),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        while let Ok(action) = receiver.try_recv() {
            push_coalesced(&mut pending, action);
        }

        let mut shutdown = false;
        for action_type in pending {
            match action_type {
                servo_action::Action::LoadUrl(load_url) => {
                    log::info!("Loading URL: {}", load_url.url);
                    if let Ok(parsed_url) = Url::parse(&load_url.url) {
                        webview.load(parsed_url);
                    }
                }
                servo_action::Action::Reload(_) => {
                    log::info!("Reloading page");
                    webview.reload();
                }
                servo_action::Action::GoBack(_) => {
                    log::info!("Going back");
                    let _ = webview.go_back(1);
                }
                servo_action::Action::GoForward(_) => {
                    log::info!("Going forward");
                    let _ = webview.go_forward(1);
                }
                servo_action::Action::Resize(resize) => {
                    log::debug!("Resizing to {}x{}", resize.width, resize.height);
                    webview.resize(PhysicalSize::new(resize.width, resize.height));
                }
                servo_action::Action::Motion(motion) => {
                    log::debug!("Mouse motion: ({}, {})", motion.x, motion.y);
                    webview.notify_input_event(InputEvent::MouseMove(MouseMoveEvent::new(
                        WebViewPoint::Device(Point2D::new(motion.x as f32, motion.y as f32)),
                    )));
                }
                servo_action::Action::ButtonPress(button_press) => {
                    log::debug!(
                        "Button press: button {} at ({}, {})",
                        button_press.button,
                        button_press.x,
                        button_press.y
                    );
                    let mouse_button = match button_press.button {
                        1 => MouseButton::Left,
                        2 => MouseButton::Middle,
                        3 => MouseButton::Right,
                        _ => MouseButton::Left,
                    };
                    webview.notify_input_event(InputEvent::MouseButton(MouseButtonEvent::new(
                        MouseButtonAction::Down,
                        mouse_button,
                        WebViewPoint::Device(Point2D::new(
                            button_press.x as f32,
                            button_press.y as f32,
                        )),
                    )));
                }
                servo_action::Action::ButtonRelease(button_release) => {
                    log::debug!(
                        "Button release: button {} at ({}, {})",
                        button_release.button,
                        button_release.x,
                        button_release.y
                    );
                    let mouse_button = match button_release.button {
                        1 => MouseButton::Left,
                        2 => MouseButton::Middle,
                        3 => MouseButton::Right,
                        _ => MouseButton::Left,
                    };
                    webview.notify_input_event(InputEvent::MouseButton(MouseButtonEvent::new(
                        MouseButtonAction::Up,
                        mouse_button,
                        WebViewPoint::Device(Point2D::new(
                            button_release.x as f32,
                            button_release.y as f32,
                        )),
                    )));
                }
                servo_action::Action::KeyPress(key_press) => {
                    log::debug!("Key press: {}", key_press.key);
                    let key_event = convert_key_event(
                        key_press.key,
                        key_press.key_type,
                        key_press.location,
                        key_press.key_code,
                        key_press.modifiers,
                        KeyState::Down,
                    );
                    webview.notify_input_event(InputEvent::Keyboard(key_event));
                }
                servo_action::Action::KeyRelease(key_release) => {
                    log::debug!("Key release: {}", key_release.key);
                    let key_event = convert_key_event(
                        key_release.key,
                        key_release.key_type,
                        key_release.location,
                        key_release.key_code,
                        key_release.modifiers,
                        KeyState::Up,
                    );
                    webview.notify_input_event(InputEvent::Keyboard(key_event));
                }
                servo_action::Action::TouchBegin(touch_begin) => {
                    log::debug!("Touch begin at ({}, {})", touch_begin.x, touch_begin.y);
                    webview.notify_input_event(InputEvent::Touch(servo::TouchEvent::new(
                        servo::TouchEventType::Down,
                        servo::TouchId(0),
                        WebViewPoint::Device(Point2D::new(
                            touch_begin.x as f32,
                            touch_begin.y as f32,
                        )),
                        servo::TouchPointerType::Touch,
                    )));
                }
                servo_action::Action::TouchUpdate(touch_update) => {
                    log::debug!("Touch update at ({}, {})", touch_update.x, touch_update.y);
                    webview.notify_input_event(InputEvent::Touch(servo::TouchEvent::new(
                        servo::TouchEventType::Move,
                        servo::TouchId(0),
                        WebViewPoint::Device(Point2D::new(
                            touch_update.x as f32,
                            touch_update.y as f32,
                        )),
                        servo::TouchPointerType::Touch,
                    )));
                }
                servo_action::Action::TouchEnd(touch_end) => {
                    log::debug!("Touch end at ({}, {})", touch_end.x, touch_end.y);
                    webview.notify_input_event(InputEvent::Touch(servo::TouchEvent::new(
                        servo::TouchEventType::Up,
                        servo::TouchId(0),
                        WebViewPoint::Device(Point2D::new(touch_end.x as f32, touch_end.y as f32)),
                        servo::TouchPointerType::Touch,
                    )));
                }
                servo_action::Action::TouchCancel(touch_cancel) => {
                    log::debug!("Touch cancel at ({}, {})", touch_cancel.x, touch_cancel.y);
                    webview.notify_input_event(InputEvent::Touch(servo::TouchEvent::new(
                        servo::TouchEventType::Cancel,
                        servo::TouchId(0),
                        WebViewPoint::Device(Point2D::new(
                            touch_cancel.x as f32,
                            touch_cancel.y as f32,
                        )),
                        servo::TouchPointerType::Touch,
                    )));
                }
                servo_action::Action::Scroll(scroll) => {
                    log::debug!("Scroll: dx={}, dy={}", scroll.dx, scroll.dy);
                    // FIXME: 20 and 10 are random numbers that appear in
                    // winit_minimal. We should properly understand it and
                    // maybe add some constants
                    webview.notify_scroll_event(
                        Scroll::Delta(WebViewVector::Device(DeviceVector2D::new(
                            20.0 * scroll.dx as f32,
                            20.0 * scroll.dy as f32,
                        ))),
                        WebViewPoint::Device(Point2D::new(10.0, 10.0)),
                    );
                }
                servo_action::Action::Shutdown(_) => {
                    log::info!("Shutting down servo");
                    shutdown = true;
                    break;
                }
            }
        }

        if shutdown {
            break;
        }

        // Spin servo event loop
        servo.spin_event_loop();
    }
}
