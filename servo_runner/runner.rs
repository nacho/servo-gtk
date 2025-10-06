/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use std::io::{self, Read, Write};
use std::rc::Rc;

use core::time::Duration;
use dpi::PhysicalSize;
use embedder_traits::resources;
use euclid::{Point2D, Size2D};
use keyboard_types::{Key, KeyState};
use servo::webrender_api::ScrollLocation;
use servo::webrender_api::units::{DeviceIntPoint, DeviceIntRect, DeviceRect, LayoutVector2D};
use servo::{
    InputEvent, KeyboardEvent, MouseButton, MouseButtonAction, MouseButtonEvent, MouseMoveEvent,
    ServoBuilder,
};
use servo::{RenderingContext, SoftwareRenderingContext, WebView, WebViewBuilder, WebViewDelegate};
use std::sync::mpsc;
use std::sync::mpsc::Receiver;
use std::thread;
use url::Url;

use servo_gtk::proto_ipc::{
    CursorChanged, FrameReady, LogLevel, LogMessage, ServoAction, ServoEvent, servo_action,
    servo_event,
};

mod resource_reader;
use resource_reader::ResourceReaderInstance;

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
    fn enabled(&self, _metadata: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
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

fn send_event(event: ServoEvent) -> std::io::Result<()> {
    let event_type = event.event.as_ref().map(|e| std::mem::discriminant(e));
    let encoded = event.encode_to_vec();
    let len = (encoded.len() as u32).to_le_bytes();
    
    log::trace!("Sending event {:?}: {} bytes", event_type, encoded.len());
    
    match io::stdout().write_all(&len) {
        Ok(_) => {
            match io::stdout().write_all(&encoded) {
                Ok(_) => {
                    match io::stdout().flush() {
                        Ok(_) => {
                            log::trace!("Event sent successfully");
                            Ok(())
                        }
                        Err(e) => {
                            log::error!("Failed to flush stdout: {:?}", e);
                            Err(e)
                        }
                    }
                }
                Err(e) => {
                    log::error!("Failed to write event data: {:?}", e);
                    Err(e)
                }
            }
        }
        Err(e) => {
            log::error!("Failed to write event length: {:?}", e);
            Err(e)
        }
    }
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
        log::trace!("Frame ready notification received");
        let size = self.rendering_context.size2d().to_i32();
        let viewport_rect = DeviceIntRect::from_origin_and_size(Point2D::origin(), size);
        
        log::trace!("Painting webview frame: {}x{}", size.width, size.height);
        webview.paint();
        self.rendering_context.present();

        match self.rendering_context.read_to_image(viewport_rect) {
            Some(rgba_image) => {
                let width = rgba_image.width();
                let height = rgba_image.height();
                let data = rgba_image.into_raw();

                log::trace!("Sending FrameReady event: {}x{}, {} bytes", width, height, data.len());
                let event = ServoEvent {
                    event: Some(servo_event::Event::FrameReady(FrameReady {
                        rgba_data: data,
                        width,
                        height,
                    })),
                };
                if let Err(e) = send_event(event) {
                    log::error!("Failed to send FrameReady event: {:?}", e);
                }
            }
            None => {
                log::warn!("Failed to read frame image from rendering context");
            }
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
        
        log::trace!("Cursor changed to: {}", cursor_str);
        let event = ServoEvent {
            event: Some(servo_event::Event::CursorChanged(CursorChanged {
                cursor: cursor_str.to_string(),
            })),
        };
        if let Err(e) = send_event(event) {
            log::error!("Failed to send CursorChanged event: {:?}", e);
        }
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
        log::info!("Starting stdin reader thread");
        let mut stdin = io::stdin();
        let mut message_count = 0u64;
        
        loop {
            let mut len_buf = [0u8; 4];
            match stdin.read_exact(&mut len_buf) {
                Ok(_) => {
                    let len = u32::from_le_bytes(len_buf) as usize;
                    log::trace!("Reading action message #{} of {} bytes", message_count + 1, len);
                    
                    if len > 10_000_000 { // 10MB sanity check
                        log::error!("Action message length {} is suspiciously large, breaking", len);
                        break;
                    }

                    let mut msg_buf = vec![0u8; len];
                    match stdin.read_exact(&mut msg_buf) {
                        Ok(_) => {
                            match ServoAction::decode_from_slice(&msg_buf) {
                                Ok(action) => {
                                    message_count += 1;
                                    let action_type = action.action.as_ref().map(|a| std::mem::discriminant(a));
                                    log::trace!("Decoded action #{}: {:?}", message_count, action_type);
                                    
                                    if tx.send(action).is_err() {
                                        log::warn!("Action receiver dropped, stopping stdin reader");
                                        break;
                                    }
                                }
                                Err(e) => {
                                    log::error!("Failed to decode action message: {:?}", e);
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            log::error!("Failed to read action data: {:?}", e);
                            break;
                        }
                    }
                }
                Err(e) => {
                    log::info!("Stdin reader ending: {:?}", e);
                    break;
                }
            }
        }
        log::info!("Stdin reader thread ended after {} messages", message_count);
    });
    rx
}

fn main() {
    let (event_logger, log_receiver) = EventLogger::new();

    log::set_logger(Box::leak(Box::new(event_logger))).expect("Failed to set logger");
    log::set_max_level(log::LevelFilter::Debug);

    init_crypto();
    resources::set(Box::new(ResourceReaderInstance::new()));

    log::info!("Starting servo runner");

    let size = PhysicalSize::new(800, 600);
    let rendering_context = Rc::new(
        SoftwareRenderingContext::new(size).expect("Failed to create Software rendering context"),
    );

    let servo_builder = ServoBuilder::new(rendering_context.clone());
    let servo = servo_builder.build();

    let delegate = Rc::new(ServoWebViewDelegate::new(rendering_context));
    let webview = WebViewBuilder::new(&servo).delegate(delegate).build();

    webview.focus_and_raise_to_top(true);

    let receiver = spawn_stdin_channel();

    let mut loop_count = 0u64;
    let mut last_log_count = 0u64;
    let mut last_action_count = 0u64;
    
    log::info!("Starting main event loop");
    loop {
        loop_count += 1;
        
        // Log periodic status every 10000 iterations
        if loop_count % 10000 == 0 {
            log::debug!("Main loop iteration {}, processed {} log messages, {} actions", 
                       loop_count, last_log_count, last_action_count);
        }

        // Process queued log messages
        let mut log_messages_processed = 0;
        while let Ok(log_message) = log_receiver.try_recv() {
            log_messages_processed += 1;
            last_log_count += 1;
            let event = ServoEvent {
                event: Some(servo_event::Event::LogMessage(log_message)),
            };
            if let Err(e) = send_event(event) {
                log::error!("Failed to send log message event: {:?}", e);
            }
        }
        
        if log_messages_processed > 0 {
            log::trace!("Processed {} log messages", log_messages_processed);
        }

        if let Ok(action) = receiver.try_recv() {
            last_action_count += 1;
            if let Some(action_type) = action.action {
                log::debug!("Processing action #{}: {:?}", last_action_count, std::mem::discriminant(&action_type));
                match action_type {
                servo_action::Action::LoadUrl(load_url) => {
                    log::info!("Loading URL: {}", load_url.url);
                    match Url::parse(&load_url.url) {
                        Ok(parsed_url) => {
                            webview.load(parsed_url);
                            log::debug!("URL load request sent to webview");
                        }
                        Err(e) => {
                            log::error!("Failed to parse URL '{}': {:?}", load_url.url, e);
                        }
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
                    webview.move_resize(DeviceRect::from_origin_and_size(
                        Point2D::origin(),
                        Size2D::new(resize.width as f32, resize.height as f32),
                    ));
                    webview.resize(PhysicalSize::new(resize.width, resize.height));
                }
                servo_action::Action::Motion(motion) => {
                    log::debug!("Mouse motion: ({}, {})", motion.x, motion.y);
                    webview.notify_input_event(InputEvent::MouseMove(MouseMoveEvent::new(
                        Point2D::new(motion.x as f32, motion.y as f32),
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
                        Point2D::new(button_press.x as f32, button_press.y as f32),
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
                        Point2D::new(button_release.x as f32, button_release.y as f32),
                    )));
                }
                servo_action::Action::KeyPress(key_press) => {
                    log::debug!("Key press: {}", key_press.key);
                    let key = Key::Character(key_press.key);
                    let key_event = KeyboardEvent::from_state_and_key(KeyState::Down, key);
                    webview.notify_input_event(InputEvent::Keyboard(key_event));
                }
                servo_action::Action::KeyRelease(key_release) => {
                    log::debug!("Key release: {}", key_release.key);
                    let key = Key::Character(key_release.key);
                    let key_event = KeyboardEvent::from_state_and_key(KeyState::Up, key);
                    webview.notify_input_event(InputEvent::Keyboard(key_event));
                }
                servo_action::Action::TouchBegin(touch_begin) => {
                    log::debug!("Touch begin at ({}, {})", touch_begin.x, touch_begin.y);
                    webview.notify_input_event(InputEvent::Touch(servo::TouchEvent::new(
                        servo::TouchEventType::Down,
                        servo::TouchId(0),
                        Point2D::new(touch_begin.x as f32, touch_begin.y as f32),
                    )));
                }
                servo_action::Action::TouchUpdate(touch_update) => {
                    log::debug!("Touch update at ({}, {})", touch_update.x, touch_update.y);
                    webview.notify_input_event(InputEvent::Touch(servo::TouchEvent::new(
                        servo::TouchEventType::Move,
                        servo::TouchId(0),
                        Point2D::new(touch_update.x as f32, touch_update.y as f32),
                    )));
                }
                servo_action::Action::TouchEnd(touch_end) => {
                    log::debug!("Touch end at ({}, {})", touch_end.x, touch_end.y);
                    webview.notify_input_event(InputEvent::Touch(servo::TouchEvent::new(
                        servo::TouchEventType::Up,
                        servo::TouchId(0),
                        Point2D::new(touch_end.x as f32, touch_end.y as f32),
                    )));
                }
                servo_action::Action::TouchCancel(touch_cancel) => {
                    log::debug!("Touch cancel at ({}, {})", touch_cancel.x, touch_cancel.y);
                    webview.notify_input_event(InputEvent::Touch(servo::TouchEvent::new(
                        servo::TouchEventType::Cancel,
                        servo::TouchId(0),
                        Point2D::new(touch_cancel.x as f32, touch_cancel.y as f32),
                    )));
                }
                servo_action::Action::Scroll(scroll) => {
                    log::debug!("Scroll: dx={}, dy={}", scroll.dx, scroll.dy);
                    // FIXME: 20 and 10 are random numbers that appear in
                    // winit_minimal. We should properly understand it and
                    // maybe add some constants
                    webview.notify_scroll_event(
                        ScrollLocation::Delta(LayoutVector2D::new(
                            20.0 * scroll.dx as f32,
                            20.0 * scroll.dy as f32,
                        )),
                        DeviceIntPoint::new(10, 10),
                    );
                }
                servo_action::Action::Shutdown(_) => {
                    log::info!("Received shutdown command, shutting down servo");
                    servo.deinit();
                    log::info!("Servo deinit complete, exiting main loop");
                    break;
                }
            }
            } else {
                log::warn!("Received action with no action type");
            }
        }

        // Spin servo event loop
        if !servo.spin_event_loop() {
            log::warn!("Servo event loop returned false, exiting");
            break;
        }

        // FIXME: we need a better way to not have a busy loop
        std::thread::sleep(Duration::from_millis(5));
    }
    
    log::info!("Main event loop ended after {} iterations, {} log messages, {} actions", 
               loop_count, last_log_count, last_action_count);
}
