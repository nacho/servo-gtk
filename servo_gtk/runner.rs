/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Servo runner subprocess entry point.

use std::cell::RefCell;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::BorrowedFd;
use std::rc::Rc;

use core::time::Duration;
use dpi::PhysicalSize;
use embedder_traits::{ContextMenuItem, WebViewPoint, WebViewVector};
use euclid::{Point2D, Scale};
use http::{HeaderMap, StatusCode};
use keyboard_types::{Code, Key, KeyState, Location, Modifiers, NamedKey};
use prost::Message;

use servo::LoadStatus;
use servo::user_contents::UserStyleSheet;
use servo::{
    AuthenticationRequest, CreateNewWebViewRequest, PermissionRequest as ServoPermissionRequest,
};
use servo::{ConsoleLogLevel, UserContentManager, UserScript};
use servo::{ContextMenu, EmbedderControl, FilePicker, SimpleDialog};
use servo::{
    DeviceIntRect, DeviceVector2D, InputEvent, KeyboardEvent, MouseButton, MouseButtonAction,
    MouseButtonEvent, MouseMoveEvent, Opts, Scroll, ServoBuilder,
};
use servo::{
    RenderingContext, SoftwareRenderingContext, WebResourceLoad, WebResourceResponse, WebView,
    WebViewBuilder, WebViewDelegate,
};
use std::str::FromStr;
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use std::thread;
use url::Url;

use crate::adblock::{AdblockState, destination_to_resource_type};
use crate::proto_ipc::{
    CursorChanged, FileChooserRequest, FrameReady, LoadEnd, LoadStart, LogLevel, LogMessage,
    PingRequest, ScriptDialogKind, ScriptDialogRequest, ScriptMessage, ServoAction, ServoEvent,
    TitleChanged, UrlChanged, servo_action, servo_event,
};

pub(crate) const SCRIPT_MESSAGE_PREFIX: &str = "__servo_gtk_msg__";

pub const RUNNER_ARG: &str = "--servo-gtk-runner";

pub fn run_if_requested() {
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
    adblock: Rc<RefCell<AdblockState>>,
    pending_dialogs: Rc<RefCell<PendingRequests<SimpleDialog>>>,
    pending_file_pickers: Rc<RefCell<PendingRequests<FilePicker>>>,
    pending_auth: Rc<RefCell<PendingRequests<AuthenticationRequest>>>,
    pending_permissions: Rc<RefCell<PendingRequests<ServoPermissionRequest>>>,
    pending_context_menus: Rc<RefCell<PendingRequests<ContextMenu>>>,
    popups: Rc<PopupState>,
}

#[derive(Default)]
struct PopupState {
    aux: RefCell<Vec<WebView>>,
}

#[derive(Default)]
struct PendingState {
    dialogs: Rc<RefCell<PendingRequests<SimpleDialog>>>,
    file_pickers: Rc<RefCell<PendingRequests<FilePicker>>>,
    auth: Rc<RefCell<PendingRequests<AuthenticationRequest>>>,
    permissions: Rc<RefCell<PendingRequests<ServoPermissionRequest>>>,
    context_menus: Rc<RefCell<PendingRequests<ContextMenu>>>,
}

impl ServoWebViewDelegate {
    fn new(
        rendering_context: Rc<dyn RenderingContext>,
        event_pipe: EventPipe,
        adblock: Rc<RefCell<AdblockState>>,
        pending: &PendingState,
        popups: Rc<PopupState>,
    ) -> Self {
        Self {
            rendering_context,
            event_pipe,
            adblock,
            pending_dialogs: pending.dialogs.clone(),
            pending_file_pickers: pending.file_pickers.clone(),
            pending_auth: pending.auth.clone(),
            pending_permissions: pending.permissions.clone(),
            pending_context_menus: pending.context_menus.clone(),
            popups,
        }
    }
}

impl WebViewDelegate for ServoWebViewDelegate {
    fn load_web_resource(&self, _webview: WebView, load: WebResourceLoad) {
        if load.request.is_for_main_frame {
            return;
        }

        let dest_debug = format!("{:?}", load.request.destination);
        if dest_debug == "Document" || dest_debug == "Frame" || dest_debug == "Iframe" {
            return;
        }

        let url = load.request.url.as_str().to_string();
        let source_url = load
            .request
            .referrer_url
            .as_ref()
            .map(|u| u.as_str().to_string());
        let resource_type = destination_to_resource_type(&dest_debug);

        if self
            .adblock
            .borrow()
            .check(&url, source_url.as_deref(), resource_type)
        {
            log::debug!("[adblock] Blocked: {} ({})", url, resource_type);

            let response = WebResourceResponse {
                url: load.request.url.clone(),
                headers: HeaderMap::new(),
                status_code: StatusCode::OK,
                status_message: Vec::new(),
            };
            let intercepted = load.intercept(response);
            intercepted.cancel();
        }
    }

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
        let url = webview.url().map(|u| u.to_string()).unwrap_or_default();
        if let Some(event) = load_status_to_event(status, url) {
            let _ = self.event_pipe.send(event);
        }
    }

    fn show_console_message(&self, _webview: WebView, _level: ConsoleLogLevel, message: String) {
        if let Some(event) = parse_script_message(&message) {
            let _ = self.event_pipe.send(event);
        }
    }

    fn request_authentication(
        &self,
        _webview: WebView,
        authentication_request: AuthenticationRequest,
    ) {
        let url = authentication_request.url().to_string();
        let for_proxy = authentication_request.for_proxy();
        let request_id = self
            .pending_auth
            .borrow_mut()
            .insert(authentication_request);
        let event = ServoEvent {
            event: Some(servo_event::Event::AuthRequest(
                crate::proto_ipc::AuthRequest {
                    request_id,
                    url,
                    for_proxy,
                },
            )),
        };
        let _ = self.event_pipe.send(event);
    }

    fn request_create_new(&self, _parent_webview: WebView, request: CreateNewWebViewRequest) {
        let webview = request.builder(self.rendering_context.clone()).build();
        self.popups.aux.borrow_mut().push(webview);
    }

    fn request_permission(&self, _webview: WebView, request: ServoPermissionRequest) {
        let feature_name = format!("{:?}", request.feature());
        let request_id = self.pending_permissions.borrow_mut().insert(request);
        let event = ServoEvent {
            event: Some(servo_event::Event::PermissionRequest(
                crate::proto_ipc::PermissionRequest {
                    request_id,
                    feature_name,
                },
            )),
        };
        let _ = self.event_pipe.send(event);
    }

    fn show_embedder_control(&self, _webview: WebView, embedder_control: EmbedderControl) {
        match embedder_control {
            EmbedderControl::SimpleDialog(dialog) => {
                let (kind, message, default_value) = match &dialog {
                    SimpleDialog::Alert(d) => (
                        ScriptDialogKind::Alert,
                        d.message().to_owned(),
                        String::new(),
                    ),
                    SimpleDialog::Confirm(d) => (
                        ScriptDialogKind::Confirm,
                        d.message().to_owned(),
                        String::new(),
                    ),
                    SimpleDialog::Prompt(d) => (
                        ScriptDialogKind::Prompt,
                        d.message().to_owned(),
                        d.current_value().to_owned(),
                    ),
                };
                let request_id = self.pending_dialogs.borrow_mut().insert(dialog);
                let event = ServoEvent {
                    event: Some(servo_event::Event::ScriptDialogRequest(
                        ScriptDialogRequest {
                            request_id,
                            kind: kind as i32,
                            message,
                            default_value,
                        },
                    )),
                };
                let _ = self.event_pipe.send(event);
            }
            EmbedderControl::FilePicker(file_picker) => {
                let allow_multiple = file_picker.allow_select_multiple();
                let filter_patterns = file_picker
                    .filter_patterns()
                    .iter()
                    .map(|pattern| pattern.0.clone())
                    .collect();
                let request_id = self.pending_file_pickers.borrow_mut().insert(file_picker);
                let event = ServoEvent {
                    event: Some(servo_event::Event::FileChooserRequest(FileChooserRequest {
                        request_id,
                        allow_multiple,
                        filter_patterns,
                    })),
                };
                let _ = self.event_pipe.send(event);
            }
            EmbedderControl::ContextMenu(context_menu) => {
                let position = context_menu.position();
                let items = context_menu
                    .items()
                    .iter()
                    .map(|item| match item {
                        ContextMenuItem::Item { label, enabled, .. } => {
                            crate::proto_ipc::ContextMenuEntry {
                                label: label.clone(),
                                enabled: *enabled,
                                separator: false,
                            }
                        }
                        ContextMenuItem::Separator => crate::proto_ipc::ContextMenuEntry {
                            label: String::new(),
                            enabled: false,
                            separator: true,
                        },
                    })
                    .collect();
                let request_id = self.pending_context_menus.borrow_mut().insert(context_menu);
                let event = ServoEvent {
                    event: Some(servo_event::Event::ContextMenuRequest(
                        crate::proto_ipc::ContextMenuRequest {
                            request_id,
                            items,
                            x: position.min.x,
                            y: position.min.y,
                        },
                    )),
                };
                let _ = self.event_pipe.send(event);
            }
            _ => {
                log::debug!("Unhandled embedder control; using default response");
            }
        }
    }
}

fn parse_script_message(message: &str) -> Option<ServoEvent> {
    let payload = message.strip_prefix(SCRIPT_MESSAGE_PREFIX)?;
    let value: serde_json::Value = serde_json::from_str(payload).ok()?;
    let name = value.get("name")?.as_str()?.to_string();
    let body = match value.get("body") {
        Some(body) => body.to_string(),
        None => "null".to_string(),
    };
    Some(ServoEvent {
        event: Some(servo_event::Event::ScriptMessage(ScriptMessage { name, body })),
    })
}

fn script_message_handler_shim(name: &str) -> String {
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
    let _code = key_code;
    let code = Code::Unidentified;
    KeyboardEvent::new_without_event(state, key, code, location, modifiers, false, false)
}

fn complete_script_dialog(dialog: SimpleDialog, answer: crate::proto_ipc::ScriptDialogResponse) {
    match dialog {
        SimpleDialog::Alert(alert) => alert.confirm(),
        SimpleDialog::Confirm(confirm) => {
            if answer.confirmed {
                confirm.confirm();
            } else {
                confirm.dismiss();
            }
        }
        SimpleDialog::Prompt(mut prompt) => {
            if answer.confirmed {
                prompt.set_current_value(&answer.value);
                prompt.confirm();
            } else {
                prompt.dismiss();
            }
        }
    }
}

fn complete_context_menu(menu: ContextMenu, answer: crate::proto_ipc::ContextMenuResponse) {
    if !answer.selected {
        menu.dismiss();
        return;
    }
    let action = menu
        .items()
        .get(answer.index as usize)
        .and_then(|item| match item {
            ContextMenuItem::Item { action, .. } => Some(*action),
            ContextMenuItem::Separator => None,
        });
    match action {
        Some(action) => menu.select(action),
        None => menu.dismiss(),
    }
}

fn complete_file_picker(mut picker: FilePicker, answer: crate::proto_ipc::FileChooserResponse) {
    if answer.paths.is_empty() {
        picker.dismiss();
    } else {
        let paths: Vec<std::path::PathBuf> = answer
            .paths
            .into_iter()
            .map(std::path::PathBuf::from)
            .collect();
        picker.select(&paths);
        picker.submit();
    }
}

fn html_to_data_url(html: &str) -> String {
    const UNRESERVED: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_.~";
    let mut out = String::from("data:text/html;charset=utf-8,");
    for &byte in html.as_bytes() {
        if UNRESERVED.contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

struct PendingRequests<T> {
    next_id: u64,
    pending: std::collections::HashMap<u64, T>,
}

impl<T> PendingRequests<T> {
    fn new() -> Self {
        Self {
            next_id: 1,
            pending: std::collections::HashMap::new(),
        }
    }

    fn insert(&mut self, request: T) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.pending.insert(id, request);
        id
    }

    fn take(&mut self, id: u64) -> Option<T> {
        self.pending.remove(&id)
    }

    /// Test-only helper. `#[allow(dead_code)]` silences `-D warnings` in
    /// non-test builds where this method has no caller.
    #[allow(dead_code)]
    fn len(&self) -> usize {
        self.pending.len()
    }
}

impl<T> Default for PendingRequests<T> {
    fn default() -> Self {
        Self::new()
    }
}

fn clamp_zoom_level(level: f64) -> f32 {
    const MIN_ZOOM: f64 = 0.1;
    const MAX_ZOOM: f64 = 10.0;
    if !level.is_finite() {
        return 1.0;
    }
    level.clamp(MIN_ZOOM, MAX_ZOOM) as f32
}

fn push_coalesced_input(pending: &mut Vec<servo_action::Action>, action: ServoAction) {
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

pub fn run() {
    const SPIN_INTERVAL: Duration = Duration::from_millis(5);

    let (event_logger, log_receiver) = EventLogger::new();

    log::set_logger(Box::leak(Box::new(event_logger))).expect("Failed to set logger");
    log::set_max_level(max_log_level());

    init_crypto();

    log::info!("Starting servo runner");

    let size = PhysicalSize::new(800, 600);
    let rendering_context = Rc::new(
        SoftwareRenderingContext::new(size).expect("Failed to create Software rendering context"),
    );

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

    let user_content_manager = Rc::new(UserContentManager::new(&servo));

    let event_pipe = EventPipe::from_stdout();
    let pending_state = PendingState::default();
    let popups = Rc::new(PopupState::default());
    let adblock = Rc::new(RefCell::new(AdblockState::new()));
    let delegate = Rc::new(ServoWebViewDelegate::new(
        rendering_context.clone(),
        event_pipe.clone(),
        adblock.clone(),
        &pending_state,
        popups.clone(),
    ));

    let webview = WebViewBuilder::new(&servo, rendering_context)
        .delegate(delegate)
        .user_content_manager(user_content_manager.clone())
        .build();

    let mut user_scripts: Vec<Rc<UserScript>> = Vec::new();
    let mut user_style_sheets: Vec<Rc<UserStyleSheet>> = Vec::new();
    let mut pending_pings: PendingRequests<()> = PendingRequests::new();

    let receiver = spawn_stdin_channel();

    'event_loop: loop {
        while let Ok(log_message) = log_receiver.try_recv() {
            let event = ServoEvent {
                event: Some(servo_event::Event::LogMessage(log_message)),
            };
            let _ = event_pipe.send(event);
        }

        let mut pending = Vec::new();
        match receiver.recv_timeout(SPIN_INTERVAL) {
            Ok(action) => push_coalesced_input(&mut pending, action),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        while let Ok(action) = receiver.try_recv() {
            push_coalesced_input(&mut pending, action);
        }

        for action_type in pending {
            match action_type {
                servo_action::Action::LoadUrl(load_url) => {
                    log::info!("Loading URL: {}", load_url.url);
                    if let Ok(parsed_url) = Url::parse(&load_url.url) {
                        webview.load(parsed_url);
                    }
                }
                servo_action::Action::LoadHtml(load_html) => {
                    log::info!("Loading HTML ({} bytes)", load_html.html.len());
                    let data_url = html_to_data_url(&load_html.html);
                    if let Ok(parsed_url) = Url::parse(&data_url) {
                        webview.load(parsed_url);
                    } else {
                        log::warn!("Failed to build data: URL for load_html");
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
                    webview.resize(PhysicalSize::new(resize.width.max(1), resize.height.max(1)));
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
                    break 'event_loop;
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
                    let shim = script_message_handler_shim(&register.name);
                    let script = Rc::new(UserScript::new(shim, None));
                    user_content_manager.add_script(script.clone());
                    user_scripts.push(script);
                }
                servo_action::Action::UnregisterScriptMessageHandler(unregister) => {
                    log::info!("Unregistering script message handler: {}", unregister.name);
                }
                servo_action::Action::EvaluateJavascript(evaluate) => {
                    log::debug!("Evaluating JavaScript ({} bytes)", evaluate.source.len());
                    webview.evaluate_javascript(evaluate.source, |_result| {});
                }
                servo_action::Action::SetZoomLevel(set_zoom_level) => {
                    let zoom = clamp_zoom_level(set_zoom_level.level);
                    log::debug!("Setting page zoom to {zoom}");
                    webview.set_page_zoom(zoom);
                }
                servo_action::Action::SetHidpiScaleFactor(set_hidpi) => {
                    let scale = set_hidpi.scale;
                    if scale.is_finite() && scale > 0.0 {
                        log::debug!("Setting HiDPI scale factor to {scale}");
                        webview.set_hidpi_scale_factor(Scale::new(scale));
                    }
                }
                servo_action::Action::UpdateAdblockRules(update) => {
                    let mut adblock_ref = adblock.borrow_mut();
                    match update.action.as_str() {
                        "load" => {
                            adblock_ref.load_rules(&update.rules);
                            log::info!("[adblock] Loaded {} rules", update.rules.len());
                        }
                        "clear" => {
                            adblock_ref.clear();
                            log::info!("[adblock] Cleared rules");
                        }
                        other => {
                            log::warn!("[adblock] Unknown action: {other}");
                        }
                    }
                }
                servo_action::Action::SetAdblockLevel(set_level) => {
                    let level = set_level.level as u8;
                    adblock.borrow().set_level(level);
                    log::info!("[adblock] Level set to {level}");
                }
                servo_action::Action::SetAdblockBlob(set_blob) => {
                    let size = set_blob.blob.len();
                    let mut adblock_ref = adblock.borrow_mut();
                    match adblock_ref.load_blob(&set_blob.blob) {
                        Ok(()) => {
                            log::info!("[adblock] Deserialized blob ({size} bytes)");
                        }
                        Err(e) => {
                            log::warn!("[adblock] Blob deserialize failed: {e}");
                        }
                    }
                }
                servo_action::Action::Ping(_) => {
                    let request_id = pending_pings.insert(());
                    log::debug!("Ping: emitting PingRequest id {request_id}");
                    let event = ServoEvent {
                        event: Some(servo_event::Event::PingRequest(PingRequest { request_id })),
                    };
                    let _ = event_pipe.send(event);
                }
                servo_action::Action::Response(response) => {
                    let request_id = response.request_id;
                    match response.payload {
                        Some(crate::proto_ipc::servo_response::Payload::Ping(ping)) => {
                            let taken = pending_pings.take(request_id);
                            match taken {
                                Some(()) => log::info!(
                                    "Ping round trip complete for id {request_id} (ok={})",
                                    ping.ok
                                ),
                                None => log::warn!(
                                    "Received ping response for unknown request id {request_id}"
                                ),
                            }
                        }
                        Some(crate::proto_ipc::servo_response::Payload::ScriptDialog(answer)) => {
                            let dialog = pending_state.dialogs.borrow_mut().take(request_id);
                            match dialog {
                                Some(dialog) => complete_script_dialog(dialog, answer),
                                None => log::warn!(
                                    "Received dialog response for unknown request id {request_id}"
                                ),
                            }
                        }
                        Some(crate::proto_ipc::servo_response::Payload::FileChooser(answer)) => {
                            let picker = pending_state.file_pickers.borrow_mut().take(request_id);
                            match picker {
                                Some(picker) => complete_file_picker(picker, answer),
                                None => log::warn!(
                                    "Received file chooser response for unknown request id {request_id}"
                                ),
                            }
                        }
                        Some(crate::proto_ipc::servo_response::Payload::Auth(answer)) => {
                            let auth = pending_state.auth.borrow_mut().take(request_id);
                            match auth {
                                Some(auth) => {
                                    if answer.confirmed {
                                        auth.authenticate(answer.username, answer.password);
                                    }
                                }
                                None => log::warn!(
                                    "Received auth response for unknown request id {request_id}"
                                ),
                            }
                        }
                        Some(crate::proto_ipc::servo_response::Payload::Permission(answer)) => {
                            let permission =
                                pending_state.permissions.borrow_mut().take(request_id);
                            match permission {
                                Some(permission) => {
                                    if answer.allow {
                                        permission.allow();
                                    } else {
                                        permission.deny();
                                    }
                                }
                                None => log::warn!(
                                    "Received permission response for unknown request id {request_id}"
                                ),
                            }
                        }
                        Some(crate::proto_ipc::servo_response::Payload::ContextMenu(answer)) => {
                            let menu = pending_state.context_menus.borrow_mut().take(request_id);
                            match menu {
                                Some(menu) => complete_context_menu(menu, answer),
                                None => log::warn!(
                                    "Received context menu response for unknown request id {request_id}"
                                ),
                            }
                        }
                        None => {
                            log::warn!("Received response with no payload for id {request_id}");
                        }
                    }
                }
            }
        }

        servo.spin_event_loop();

        {
            let mut aux = popups.aux.borrow_mut();
            aux.retain(|webview| match webview.url() {
                Some(url) => {
                    let url = url.to_string();
                    if url.is_empty() || url == "about:blank" {
                        return true;
                    }
                    log::info!("Popup requested; surfacing new WebView for {url}");
                    let event = ServoEvent {
                        event: Some(servo_event::Event::CreateWebView(
                            crate::proto_ipc::CreateWebView { url },
                        )),
                    };
                    let _ = event_pipe.send(event);
                    false
                }
                None => true,
            });
        }
    }
}
