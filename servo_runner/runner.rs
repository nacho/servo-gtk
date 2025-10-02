use std::io::{self, Write};
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

use servo_gtk::ipc::{ServoAction, ServoEvent};
use servo_gtk::resources::ResourceReaderInstance;

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

            let event = ServoEvent::FrameReady(data, width, height);
            if let Ok(json) = serde_json::to_string(&event) {
                println!("{}", json);
                io::stdout().flush().unwrap();
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
        let event = ServoEvent::CursorChanged(cursor_str.to_string());
        if let Ok(json) = serde_json::to_string(&event) {
            println!("{}", json);
            io::stdout().flush().unwrap();
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
        loop {
            let mut buffer = String::new();
            io::stdin().read_line(&mut buffer).unwrap();
            if let Ok(action) = serde_json::from_str::<ServoAction>(buffer.trim()) {
                tx.send(action).unwrap();
            }
        }
    });
    rx
}

fn main() {
    init_crypto();
    resources::set(Box::new(ResourceReaderInstance::new()));

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

    loop {
        if let Ok(action) = receiver.try_recv() {
            match action {
                ServoAction::LoadUrl(url) => {
                    if let Ok(parsed_url) = Url::parse(&url) {
                        webview.load(parsed_url);
                    }
                }
                ServoAction::Reload => {
                    webview.reload();
                }
                ServoAction::GoBack => {
                    let _ = webview.go_back(1);
                }
                ServoAction::GoForward => {
                    let _ = webview.go_forward(1);
                }
                ServoAction::Resize(width, height) => {
                    webview.move_resize(DeviceRect::from_origin_and_size(
                        Point2D::origin(),
                        Size2D::new(width as f32, height as f32),
                    ));
                    webview.resize(PhysicalSize::new(width, height));
                }
                ServoAction::Motion(x, y) => {
                    webview.notify_input_event(InputEvent::MouseMove(MouseMoveEvent::new(
                        Point2D::new(x as f32, y as f32),
                    )));
                }
                ServoAction::ButtonPress(button, x, y) => {
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
                    let key = Key::Character(keyval.into());
                    let key_event = KeyboardEvent::from_state_and_key(KeyState::Down, key);
                    webview.notify_input_event(InputEvent::Keyboard(key_event));
                }
                ServoAction::KeyRelease(keyval) => {
                    let key = Key::Character(keyval.into());
                    let key_event = KeyboardEvent::from_state_and_key(KeyState::Up, key);
                    webview.notify_input_event(InputEvent::Keyboard(key_event));
                }
                ServoAction::TouchBegin(x, y) => {
                    webview.notify_input_event(InputEvent::Touch(servo::TouchEvent::new(
                        servo::TouchEventType::Down,
                        servo::TouchId(0),
                        Point2D::new(x as f32, y as f32),
                    )));
                }
                ServoAction::TouchUpdate(x, y) => {
                    webview.notify_input_event(InputEvent::Touch(servo::TouchEvent::new(
                        servo::TouchEventType::Move,
                        servo::TouchId(0),
                        Point2D::new(x as f32, y as f32),
                    )));
                }
                ServoAction::TouchEnd(x, y) => {
                    webview.notify_input_event(InputEvent::Touch(servo::TouchEvent::new(
                        servo::TouchEventType::Up,
                        servo::TouchId(0),
                        Point2D::new(x as f32, y as f32),
                    )));
                }
                ServoAction::TouchCancel(x, y) => {
                    webview.notify_input_event(InputEvent::Touch(servo::TouchEvent::new(
                        servo::TouchEventType::Cancel,
                        servo::TouchId(0),
                        Point2D::new(x as f32, y as f32),
                    )));
                }

                ServoAction::Scroll(delta_x, delta_y) => {
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
                ServoAction::Shutdown => {
                    servo.deinit();
                    break;
                }
            }
        }

        // Spin servo event loop
        if !servo.spin_event_loop() {
            break;
        }

        // FIXME: we need a better way to not have a busy loop
        std::thread::sleep(Duration::from_millis(5));
    }
}
