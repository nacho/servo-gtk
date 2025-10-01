/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use async_channel;
use dpi::PhysicalSize;
use embedder_traits::resources;
use euclid::{Point2D, Size2D};
use glib::{debug, info, warn};
use image::RgbaImage;
use keyboard_types::{Key, KeyState};
use servo::webrender_api::ScrollLocation;
use servo::webrender_api::units::{DeviceIntPoint, DeviceIntRect, DeviceRect, LayoutVector2D};
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
    Reload,
    GoBack,
    GoForward,
    Resize(u32, u32),
    Motion(f64, f64),
    ButtonPress(u32, f64, f64),
    ButtonRelease(u32, f64, f64),
    KeyPress(char),
    KeyRelease(char),
    TouchBegin(f64, f64),
    TouchUpdate(f64, f64),
    TouchEnd(f64, f64),
    TouchCancel(f64, f64),
    Scroll(f64, f64),
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

    pub fn reload(&self) {
        let _ = self.sender.send(ServoAction::Reload);
    }

    pub fn go_back(&self) {
        let _ = self.sender.send(ServoAction::GoBack);
    }

    pub fn go_forward(&self) {
        let _ = self.sender.send(ServoAction::GoForward);
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

    pub fn touch_begin(&self, x: f64, y: f64) {
        let _ = self.sender.send(ServoAction::TouchBegin(x, y));
    }

    pub fn touch_update(&self, x: f64, y: f64) {
        let _ = self.sender.send(ServoAction::TouchUpdate(x, y));
    }

    pub fn touch_end(&self, x: f64, y: f64) {
        let _ = self.sender.send(ServoAction::TouchEnd(x, y));
    }

    pub fn touch_cancel(&self, x: f64, y: f64) {
        let _ = self.sender.send(ServoAction::TouchCancel(x, y));
    }

    pub fn scroll(&self, delta_x: f64, delta_y: f64) {
        let _ = self.sender.send(ServoAction::Scroll(delta_x, delta_y));
    }

    pub fn shutdown(&self) {
        let _ = self.sender.send(ServoAction::Shutdown);
    }

    fn run_servo(receiver: Receiver<ServoAction>, event_sender: async_channel::Sender<ServoEvent>) {
        info!("Servo thread running");

        init_crypto();

        debug!("Loading resources from gresource");
        resources::set(Box::new(ResourceReaderInstance::new()));

        let size = PhysicalSize::new(800, 600);
        let rendering_context = Rc::new(
            SoftwareRenderingContext::new(size)
                .expect("Failed to create Software rendering context"),
        );

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
                    ServoAction::Reload => {
                        info!("Reloading page");
                        webview.reload();
                    }
                    ServoAction::GoBack => {
                        info!("Going back");
                        webview.go_back(1);
                    }
                    ServoAction::GoForward => {
                        info!("Going forward");
                        webview.go_forward(1);
                    }
                    ServoAction::Resize(width, height) => {
                        info!("Resizing to: {}x{}", width, height);
                        webview.move_resize(DeviceRect::from_origin_and_size(
                            Point2D::origin(),
                            Size2D::new(width as f32, height as f32),
                        ));
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
                        webview.notify_input_event(InputEvent::MouseButton(MouseButtonEvent::new(
                            MouseButtonAction::Click,
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
                    ServoAction::TouchBegin(x, y) => {
                        info!("Touch begin: x={}, y={}", x, y);
                        webview.notify_input_event(InputEvent::Touch(servo::TouchEvent::new(
                            servo::TouchEventType::Down,
                            servo::TouchId(0),
                            Point2D::new(x as f32, y as f32),
                        )));
                    }
                    ServoAction::TouchUpdate(x, y) => {
                        info!("Touch update: x={}, y={}", x, y);
                        webview.notify_input_event(InputEvent::Touch(servo::TouchEvent::new(
                            servo::TouchEventType::Move,
                            servo::TouchId(0),
                            Point2D::new(x as f32, y as f32),
                        )));
                    }
                    ServoAction::TouchEnd(x, y) => {
                        info!("Touch end: x={}, y={}", x, y);
                        webview.notify_input_event(InputEvent::Touch(servo::TouchEvent::new(
                            servo::TouchEventType::Up,
                            servo::TouchId(0),
                            Point2D::new(x as f32, y as f32),
                        )));
                    }
                    ServoAction::TouchCancel(x, y) => {
                        info!("Touch cancel: x={}, y={}", x, y);
                        webview.notify_input_event(InputEvent::Touch(servo::TouchEvent::new(
                            servo::TouchEventType::Cancel,
                            servo::TouchId(0),
                            Point2D::new(x as f32, y as f32),
                        )));
                    }
                    ServoAction::Scroll(delta_x, delta_y) => {
                        info!("Scroll: delta_x={}, delta_y={}", delta_x, delta_y);
                        // FIXME: 20 and 10 are random numbers that appear in
                        // winit_minimal. We should properly understand it and
                        // maybe add some constants
                        webview.notify_scroll_event(
                            ScrollLocation::Delta(LayoutVector2D::new(
                                20.0 * delta_x as f32,
                                20.0 * delta_y as f32,
                            )),
                            DeviceIntPoint::new(10, 10),
                        );
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
