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

use core::time::Duration;
use dpi::PhysicalSize;
use embedder_traits::{WebViewPoint, WebViewVector};
use euclid::Point2D;
use keyboard_types::{Code, Key, KeyState, Location, Modifiers, NamedKey};
use prost::Message;

use servo::LoadStatus;
use servo::user_contents::UserStyleSheet;
use servo::{ConsoleLogLevel, UserContentManager, UserScript};
use servo::{
    DeviceIntRect, DeviceVector2D, InputEvent, KeyboardEvent, MouseButton, MouseButtonAction,
    MouseButtonEvent, MouseMoveEvent, Opts, Scroll, ServoBuilder,
};
use servo::{RenderingContext, SoftwareRenderingContext, WebView, WebViewBuilder, WebViewDelegate};
use std::str::FromStr;
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use std::thread;
use url::Url;

use crate::proto_ipc::{
    CursorChanged, FrameReady, LoadEnd, LoadStart, LogLevel, LogMessage, ScriptMessage,
    ServoAction, ServoEvent, TitleChanged, UrlChanged, servo_action, servo_event,
};

/// Prefix used by the injected script-message shim when forwarding a page
/// message to the native side over the console-message bridge. Page messages
/// are emitted as `console.log("<PREFIX>" + json)` where `json` is
/// `{"name":<handler>,"body":<value>}`, and the runner detects this prefix in
/// [`ServoWebViewDelegate::show_console_message`] to re-emit them as
/// [`ScriptMessage`] events.
pub(crate) const SCRIPT_MESSAGE_PREFIX: &str = "__servo_gtk_msg__";

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
/// This defaults to `Warn` as the logging is very expensive.
fn max_log_level() -> log::LevelFilter {
    match std::env::var("SERVO_GTK_LOG") {
        Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
            "off" => log::LevelFilter::Off,
            "error" => log::LevelFilter::Error,
            "warn" | "warning" => log::LevelFilter::Warn,
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
/// writes straight through instead of buffering.
///
/// This is intentionally made !Send, so that only one thread can actually write
/// to stdout (unless `io::stdout()` is used, of course).
#[derive(Clone)]
struct EventPipe {
    file: Rc<File>,
}

impl EventPipe {
    fn from_stdout() -> Self {
        let stdout = unsafe { BorrowedFd::borrow_raw(1) };
        let owned = stdout
            .try_clone_to_owned()
            .expect("Failed to duplicate stdout");

        Self {
            file: Rc::new(File::from(owned)),
        }
    }

    fn send(&self, event: ServoEvent) -> std::io::Result<()> {
        let encoded = event.encode_to_vec();
        let len = (encoded.len() as u32).to_le_bytes();

        let mut file = &*self.file;
        file.write_all(&len)?;
        file.write_all(&encoded)
    }
}

struct ServoWebViewDelegate {
    rendering_context: Rc<dyn RenderingContext>,
    event_pipe: EventPipe,
}

impl ServoWebViewDelegate {
    fn new(rendering_context: Rc<dyn RenderingContext>, event_pipe: EventPipe) -> Self {
        Self {
            rendering_context,
            event_pipe,
        }
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
            let _ = self.event_pipe.send(event);
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
        let _ = self.event_pipe.send(event);
    }

    fn notify_url_changed(&self, _webview: WebView, url: Url) {
        let event = ServoEvent {
            event: Some(servo_event::Event::UrlChanged(UrlChanged {
                url: url.to_string(),
            })),
        };
        let _ = self.event_pipe.send(event);
    }

    fn notify_page_title_changed(&self, _webview: WebView, title: Option<String>) {
        let event = ServoEvent {
            event: Some(servo_event::Event::TitleChanged(TitleChanged {
                title: title.unwrap_or_default(),
            })),
        };
        let _ = self.event_pipe.send(event);
    }

    fn notify_load_status_changed(&self, webview: WebView, status: LoadStatus) {
        // Use the current URL of the webview as the payload; it may be `None`
        // very early in a load, in which case we send an empty string.
        let url = webview.url().map(|u| u.to_string()).unwrap_or_default();
        if let Some(event) = load_status_to_event(status, url) {
            let _ = self.event_pipe.send(event);
        }
    }

    fn show_console_message(&self, _webview: WebView, _level: ConsoleLogLevel, message: String) {
        // Page->native message channel bridge: injected handler shims forward
        // messages as prefixed console messages. Detect them here and re-emit
        // as ScriptMessage events. Non-matching console output is ignored (the
        // separate log-message plumbing handles normal logging).
        if let Some(event) = parse_script_message(&message) {
            let _ = self.event_pipe.send(event);
        }
    }
}

/// Parse a console message produced by the injected script-message shim into a
/// [`ScriptMessage`] event, or `None` if it is not a script message.
///
/// The shim emits `"<SCRIPT_MESSAGE_PREFIX>" + JSON.stringify({name, body})`
/// where `body` is itself the JSON serialization of the value the page passed
/// to `postMessage`. We forward `name` and the raw JSON `body` string.
fn parse_script_message(message: &str) -> Option<ServoEvent> {
    let payload = message.strip_prefix(SCRIPT_MESSAGE_PREFIX)?;
    let value: serde_json::Value = serde_json::from_str(payload).ok()?;
    let name = value.get("name")?.as_str()?.to_string();
    // `body` may be any JSON value; forward its serialized form as a string.
    let body = match value.get("body") {
        Some(body) => body.to_string(),
        None => "null".to_string(),
    };
    Some(ServoEvent {
        event: Some(servo_event::Event::ScriptMessage(ScriptMessage {
            name,
            body,
        })),
    })
}

/// Build the JavaScript source for the message-handler shim registered for
/// `name`. Injected as a user script, it ensures
/// `window.servoGtk.messageHandlers.<name>.postMessage(value)` forwards the
/// value to the native side over the console-message bridge.
fn script_message_handler_shim(name: &str) -> String {
    // `name` is embedded as a JSON string literal to guard against injection
    // and to preserve exact handler names.
    let name_literal = serde_json::to_string(name).unwrap_or_else(|_| "\"\"".to_string());
    let prefix_literal = serde_json::to_string(SCRIPT_MESSAGE_PREFIX)
        .unwrap_or_else(|_| "\"__servo_gtk_msg__\"".to_string());
    format!(
        r#"(function() {{
  window.servoGtk = window.servoGtk || {{}};
  window.servoGtk.messageHandlers = window.servoGtk.messageHandlers || {{}};
  var name = {name_literal};
  window.servoGtk.messageHandlers[name] = {{
    postMessage: function(value) {{
      console.log({prefix_literal} + JSON.stringify({{ name: name, body: value }}));
    }}
  }};
}})();"#
    )
}

/// Map a Servo [`LoadStatus`] to the corresponding [`ServoEvent`], if any.
///
/// Per the servo-gtk IPC contract we only surface the start and completion of
/// a load: `Started` becomes a [`LoadStart`] and `Complete` becomes a
/// [`LoadEnd`]. The intermediate `HeadParsed` state has no dedicated event and
/// maps to `None`.
fn load_status_to_event(status: LoadStatus, url: String) -> Option<ServoEvent> {
    match status {
        LoadStatus::Started => Some(ServoEvent {
            event: Some(servo_event::Event::LoadStart(LoadStart { url })),
        }),
        LoadStatus::Complete => Some(ServoEvent {
            event: Some(servo_event::Event::LoadEnd(LoadEnd { url })),
        }),
        LoadStatus::HeadParsed => None,
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

    // Give Servo a persistent, per-user config directory. Without this, Servo
    // falls back to a throwaway `tempfile` directory under /tmp for its storage
    // engines (ClientStorage, WebStorage, cache), which is non-persistent and
    // prone to a "unable to open database file" (SqliteFailure CannotOpen)
    // warning when that temp dir is reaped before the storage thread opens it.
    //
    // We mirror servoshell's behaviour: compute a stable config dir, ensure it
    // exists, and pass it through `Opts`. `glib::user_config_dir()` wraps
    // `g_get_user_config_dir()` (respects XDG_CONFIG_HOME, defaults to
    // ~/.config on Linux).
    let config_dir = glib::user_config_dir().join("servo-gtk");
    if let Err(error) = std::fs::create_dir_all(&config_dir) {
        log::warn!("Failed to create servo-gtk config dir {config_dir:?}: {error}");
    }

    let opts = Opts {
        config_dir: Some(config_dir),
        ..Default::default()
    };
    let servo_builder = ServoBuilder::default().opts(opts);
    let servo = servo_builder.build();

    // User content manager: backs the parent-side servo_gtk::UserContentManager
    // API (user-script/style injection and the page->native message channel).
    let user_content_manager = Rc::new(UserContentManager::new(&servo));

    let event_pipe = EventPipe::from_stdout();
    let delegate = Rc::new(ServoWebViewDelegate::new(
        rendering_context.clone(),
        event_pipe.clone(),
    ));

    let webview = WebViewBuilder::new(&servo, rendering_context)
        .delegate(delegate)
        .user_content_manager(user_content_manager.clone())
        .build();

    // Track the scripts and stylesheets we have added so that we can implement
    // the bulk `remove_all_*` operations, which Servo's UserContentManager does
    // not provide natively (it only supports removal by individual handle).
    let mut user_scripts: Vec<Rc<UserScript>> = Vec::new();
    let mut user_style_sheets: Vec<Rc<UserStyleSheet>> = Vec::new();

    let receiver = spawn_stdin_channel();

    loop {
        // Process queued log messages
        while let Ok(log_message) = log_receiver.try_recv() {
            let event = ServoEvent {
                event: Some(servo_event::Event::LogMessage(log_message)),
            };
            let _ = event_pipe.send(event);
        }

        if let Ok(action) = receiver.try_recv()
            && let Some(action_type) = action.action
        {
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
                    break;
                }
                servo_action::Action::AddUserScript(add_user_script) => {
                    log::info!(
                        "Adding user script ({} bytes)",
                        add_user_script.source.len()
                    );
                    let script = Rc::new(UserScript::new(add_user_script.source, None));
                    user_content_manager.add_script(script.clone());
                    user_scripts.push(script);
                }
                servo_action::Action::AddUserStyleSheet(add_user_style_sheet) => {
                    log::info!(
                        "Adding user style sheet ({} bytes)",
                        add_user_style_sheet.source.len()
                    );
                    // Servo requires a URL to identify the stylesheet's origin;
                    // a synthetic about: URL is sufficient for injected styles.
                    if let Ok(url) = Url::parse("about:user-stylesheet") {
                        let stylesheet =
                            Rc::new(UserStyleSheet::new(add_user_style_sheet.source, url));
                        user_content_manager.add_stylesheet(stylesheet.clone());
                        user_style_sheets.push(stylesheet);
                    }
                }
                servo_action::Action::RemoveAllUserScripts(_) => {
                    log::info!("Removing all user scripts");
                    for script in user_scripts.drain(..) {
                        user_content_manager.remove_script(script);
                    }
                }
                servo_action::Action::RemoveAllUserStyleSheets(_) => {
                    log::info!("Removing all user style sheets");
                    for stylesheet in user_style_sheets.drain(..) {
                        user_content_manager.remove_stylesheet(stylesheet);
                    }
                }
                servo_action::Action::RegisterScriptMessageHandler(register) => {
                    log::info!("Registering script message handler: {}", register.name);
                    // Inject a shim user script that defines
                    // window.servoGtk.messageHandlers.<name>.postMessage and
                    // forwards to native over the console-message bridge.
                    let shim = script_message_handler_shim(&register.name);
                    let script = Rc::new(UserScript::new(shim, None));
                    user_content_manager.add_script(script.clone());
                    user_scripts.push(script);
                }
                servo_action::Action::UnregisterScriptMessageHandler(unregister) => {
                    log::info!("Unregistering script message handler: {}", unregister.name);
                    // Servo cannot un-inject an already-applied user script, and
                    // does not expose per-name script identity. The handler will
                    // stop being injected once scripts are cleared/reloaded; for
                    // now this is a no-op beyond logging. Documented divergence.
                }
                servo_action::Action::EvaluateJavascript(evaluate) => {
                    log::debug!("Evaluating JavaScript ({} bytes)", evaluate.source.len());
                    // Fire-and-forget: the result is not currently returned to
                    // the native side.
                    webview.evaluate_javascript(evaluate.source, |_result| {});
                }
            }
        }

        // Spin servo event loop
        servo.spin_event_loop();

        // FIXME: we need a better way to not have a busy loop
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_status_started_maps_to_load_start() {
        let event = load_status_to_event(LoadStatus::Started, "https://example.com/".to_string());
        match event.and_then(|e| e.event) {
            Some(servo_event::Event::LoadStart(load_start)) => {
                assert_eq!(load_start.url, "https://example.com/");
            }
            other => panic!("expected LoadStart, got {other:?}"),
        }
    }

    #[test]
    fn load_status_complete_maps_to_load_end() {
        let event = load_status_to_event(LoadStatus::Complete, "https://example.com/".to_string());
        match event.and_then(|e| e.event) {
            Some(servo_event::Event::LoadEnd(load_end)) => {
                assert_eq!(load_end.url, "https://example.com/");
            }
            other => panic!("expected LoadEnd, got {other:?}"),
        }
    }

    #[test]
    fn load_status_head_parsed_maps_to_none() {
        let event =
            load_status_to_event(LoadStatus::HeadParsed, "https://example.com/".to_string());
        assert!(event.is_none());
    }

    #[test]
    fn parse_script_message_ignores_unprefixed_console_output() {
        assert!(parse_script_message("just a normal log line").is_none());
        assert!(parse_script_message("").is_none());
    }

    #[test]
    fn parse_script_message_extracts_name_and_body_object() {
        let payload = format!(
            "{SCRIPT_MESSAGE_PREFIX}{}",
            r#"{"name":"auth","body":{"type":"TOKEN","value":"abc"}}"#
        );
        match parse_script_message(&payload).and_then(|e| e.event) {
            Some(servo_event::Event::ScriptMessage(msg)) => {
                assert_eq!(msg.name, "auth");
                // body is forwarded as its JSON serialization
                let parsed: serde_json::Value = serde_json::from_str(&msg.body).unwrap();
                assert_eq!(parsed["type"], "TOKEN");
                assert_eq!(parsed["value"], "abc");
            }
            other => panic!("expected ScriptMessage, got {other:?}"),
        }
    }

    #[test]
    fn parse_script_message_handles_string_body() {
        let payload = format!(
            "{SCRIPT_MESSAGE_PREFIX}{}",
            r#"{"name":"h","body":"hello"}"#
        );
        match parse_script_message(&payload).and_then(|e| e.event) {
            Some(servo_event::Event::ScriptMessage(msg)) => {
                assert_eq!(msg.name, "h");
                assert_eq!(msg.body, "\"hello\"");
            }
            other => panic!("expected ScriptMessage, got {other:?}"),
        }
    }

    #[test]
    fn parse_script_message_rejects_prefixed_but_invalid_json() {
        let payload = format!("{SCRIPT_MESSAGE_PREFIX}not json");
        assert!(parse_script_message(&payload).is_none());
    }

    #[test]
    fn parse_script_message_requires_name() {
        let payload = format!("{SCRIPT_MESSAGE_PREFIX}{}", r#"{"body":1}"#);
        assert!(parse_script_message(&payload).is_none());
    }

    #[test]
    fn shim_embeds_handler_name_and_prefix() {
        let shim = script_message_handler_shim("myHandler");
        assert!(shim.contains("window.servoGtk"));
        assert!(shim.contains("messageHandlers"));
        assert!(shim.contains("\"myHandler\""));
        assert!(shim.contains(SCRIPT_MESSAGE_PREFIX));
        assert!(shim.contains("postMessage"));
    }

    #[test]
    fn shim_escapes_quotes_in_handler_name() {
        // A handler name containing a quote must not break out of the string
        // literal in the generated JS.
        let shim = script_message_handler_shim("a\"b");
        assert!(shim.contains(r#""a\"b""#));
    }
}
