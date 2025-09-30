/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use async_channel;
use dpi::PhysicalSize;
use embedder_traits::resources;
use euclid::Point2D;
use glib::{debug, info, warn};
use image::RgbaImage;
use keyboard_types::{Key, KeyState};
use servo::webrender_api::units::DeviceIntRect;
use servo::{
    InputEvent, KeyboardEvent, MouseButton, MouseButtonAction, MouseButtonEvent, MouseMoveEvent,
    ServoBuilder,
};
use servo::{RenderingContext, SoftwareRenderingContext, WebView, WebViewBuilder, WebViewDelegate};
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use url::Url;

use crate::resources::ResourceReaderInstance;

const G_LOG_DOMAIN: &str = "ServoGtk";

pub enum ServoAction {
    LoadUrl(String),
    Resize(u32, u32),
    Motion(f64, f64),
    ButtonPress(u32, f64, f64),
    ButtonRelease(u32, f64, f64),
    KeyPress(char),
    KeyRelease(char),
    Shutdown,
}

pub enum ServoEvent {
    FrameReady(RgbaImage),
    LoadComplete,
    CursorChanged(servo::Cursor),
}

pub struct ServoRunner {
    sender: Sender<ServoAction>,
    event_receiver: async_channel::Receiver<ServoEvent>,
}

struct ServoWebViewDelegate {
    event_sender: async_channel::Sender<ServoEvent>,
    rendering_context: Rc<dyn RenderingContext>,
}

impl ServoWebViewDelegate {
    pub(crate) fn new(
        event_sender: async_channel::Sender<ServoEvent>,
        rendering_context: Rc<dyn RenderingContext>,
    ) -> Self {
        Self {
            event_sender,
            rendering_context,
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
            if let Err(e) = self
                .event_sender
                .send_blocking(ServoEvent::FrameReady(rgba_image.clone()))
            {
                warn!("Could not set the pixels: {e}");
            }
        }
    }

    fn notify_cursor_changed(&self, _webview: servo::WebView, cursor: servo::Cursor) {
        let _ = self
            .event_sender
            .send_blocking(ServoEvent::CursorChanged(cursor));
    }
}

impl ServoRunner {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        let (event_sender, event_receiver) = async_channel::unbounded();

        thread::spawn(move || {
            Self::run_servo(receiver, event_sender);
        });

        Self {
            sender,
            event_receiver,
        }
    }

    pub fn event_receiver(&self) -> &async_channel::Receiver<ServoEvent> {
        &self.event_receiver
    }

    pub fn load_url(&self, url: &str) {
        let _ = self.sender.send(ServoAction::LoadUrl(url.to_string()));
    }

    pub fn resize(&self, width: u32, height: u32) {
        let _ = self.sender.send(ServoAction::Resize(width, height));
    }

    pub fn motion(&self, x: f64, y: f64) {
        let _ = self.sender.send(ServoAction::Motion(x, y));
    }

    pub fn button_press(&self, button: u32, x: f64, y: f64) {
        let _ = self.sender.send(ServoAction::ButtonPress(button, x, y));
    }

    pub fn button_release(&self, button: u32, x: f64, y: f64) {
        let _ = self.sender.send(ServoAction::ButtonRelease(button, x, y));
    }

    pub fn key_press(&self, keyval: char) {
        let _ = self.sender.send(ServoAction::KeyPress(keyval));
    }

    pub fn key_release(&self, keyval: char) {
        let _ = self.sender.send(ServoAction::KeyRelease(keyval));
    }

    pub fn shutdown(&self) {
        let _ = self.sender.send(ServoAction::Shutdown);
    }

    fn run_servo(receiver: Receiver<ServoAction>, event_sender: async_channel::Sender<ServoEvent>) {
        info!("Servo thread running");

        init_crypto();
        // FIXME: This should be taken from the system path instead of relative to executable
        debug!("Loading resources from gresource");
        resources::set(Box::new(ResourceReaderInstance::new()));

        // Create rendering context with initial size (matching servoshell pattern)
        let size = PhysicalSize::new(800, 600);
        let rendering_context = Rc::new(
            SoftwareRenderingContext::new(size)
                .expect("Failed to create Software rendering context"),
        );

        // Use ServoBuilder pattern like servoshell
        let servo_builder = ServoBuilder::new(rendering_context.clone());

        let servo = servo_builder.build();

        let delegate = ServoWebViewDelegate::new(event_sender, rendering_context);
        let webview = WebViewBuilder::new(&servo)
            .delegate(Rc::new(delegate))
            .build();

        webview.focus_and_raise_to_top(true);

        loop {
            if let Ok(command) = receiver.try_recv() {
                match command {
                    ServoAction::LoadUrl(url) => {
                        if let Ok(parsed_url) = Url::parse(&url) {
                            info!("Loading URL: {}", url);
                            webview.load(parsed_url);
                        } else {
                            warn!("Invalid URL");
                        }
                    }
                    ServoAction::Resize(width, height) => {
                        info!("Resizing to: {}x{}", width, height);
                        webview.resize(PhysicalSize::new(width, height));
                    }
                    ServoAction::Motion(x, y) => {
                        debug!("Motion: x={}, y={}", x, y);
                        webview.notify_input_event(InputEvent::MouseMove(MouseMoveEvent::new(
                            Point2D::new(x as f32, y as f32),
                        )));
                    }
                    ServoAction::ButtonPress(button, x, y) => {
                        info!("Button press: button={}, x={}, y={}", button, x, y);
                        let mouse_button = match button {
                            1 => MouseButton::Left,
                            2 => MouseButton::Middle,
                            3 => MouseButton::Right,
                            _ => MouseButton::Left,
                        };
                        webview.notify_input_event(InputEvent::MouseButton(MouseButtonEvent::new(
                            MouseButtonAction::Down,
                            mouse_button,
                            Point2D::new(x as f32, y as f32),
                        )));
                    }
                    ServoAction::ButtonRelease(button, x, y) => {
                        info!("Button release: button={}, x={}, y={}", button, x, y);
                        let mouse_button = match button {
                            1 => MouseButton::Left,
                            2 => MouseButton::Middle,
                            3 => MouseButton::Right,
                            _ => MouseButton::Left,
                        };
                        webview.notify_input_event(InputEvent::MouseButton(MouseButtonEvent::new(
                            MouseButtonAction::Up,
                            mouse_button,
                            Point2D::new(x as f32, y as f32),
                        )));
                    }
                    ServoAction::KeyPress(keyval) => {
                        info!("Key press: keyval={}", keyval);
                        let key = Key::Character(keyval.into());
                        let key_event = KeyboardEvent::from_state_and_key(KeyState::Down, key);
                        webview.notify_input_event(InputEvent::Keyboard(key_event));
                    }
                    ServoAction::KeyRelease(keyval) => {
                        info!("Key release: keyval={}", keyval);
                        let key = Key::Character(keyval.into());
                        let key_event = KeyboardEvent::from_state_and_key(KeyState::Up, key);
                        webview.notify_input_event(InputEvent::Keyboard(key_event));
                    }
                    ServoAction::Shutdown => break,
                }
            }

            if !servo.spin_event_loop() {
                break;
            }
        }
    }
}

fn init_crypto() {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("Error initializing crypto provider");
}
