/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use crate::proto_ipc::{ServoEvent, servo_event};
use crate::servo_runner::{LogLevel, ServoRunner};
use glib::info;
use glib::translate::*;
use gtk::gdk;
use gtk::prelude::*;
use gtk::{glib, subclass::prelude::*};
use image::RgbaImage;
use std::cell::RefCell;

const G_LOG_DOMAIN: &str = "ServoGtk";

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct WebView {
        pub servo_runner: RefCell<Option<ServoRunner>>,
        pub memory_texture: RefCell<Option<gdk::MemoryTexture>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for WebView {
        const NAME: &'static str = "WebView";
        type Type = super::WebView;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for WebView {
        fn constructed(&self) {
            self.parent_constructed();

            let servo_runner = ServoRunner::new();
            let event_receiver = servo_runner.event_receiver();

            self.servo_runner.replace(Some(servo_runner));

            let obj_weak = self.obj().downgrade();
            glib::spawn_future_local(async move {
                while let Ok(event) = event_receiver.recv().await {
                    if let Some(obj) = obj_weak.upgrade() {
                        obj.process_servo_event(event);
                    } else {
                        break;
                    }
                }
            });

            // Event controllers
            let motion_controller = gtk::EventControllerMotion::new();
            let obj_weak = self.obj().downgrade();
            motion_controller.connect_motion(move |_, x, y| {
                if let Some(obj) = obj_weak.upgrade() {
                    let imp = obj.imp();
                    if let Some(servo) = imp.servo_runner.borrow().as_ref() {
                        servo.motion(x, y);
                    }
                }
            });
            self.obj().add_controller(motion_controller);

            let legacy_controller = gtk::EventControllerLegacy::new();
            let obj_weak = self.obj().downgrade();
            legacy_controller.connect_event(move |controller, event| {
                if let Some(obj) = obj_weak.upgrade() {
                    let imp = obj.imp();
                    if let Some(servo) = imp.servo_runner.borrow().as_ref()
                        && let Some((x, y)) = obj.translate_event_coordinates(event)
                    {
                        match event.event_type() {
                            gdk::EventType::ButtonPress => {
                                if let Some(button_event) = event.downcast_ref::<gdk::ButtonEvent>()
                                {
                                    servo.button_press(button_event.button(), x, y);
                                }
                                controller.widget().expect("Controller widget").grab_focus();
                            }
                            gdk::EventType::ButtonRelease => {
                                if let Some(button_event) = event.downcast_ref::<gdk::ButtonEvent>()
                                {
                                    servo.button_release(button_event.button(), x, y);
                                }
                            }
                            gdk::EventType::TouchBegin => {
                                servo.touch_begin(x, y);
                                controller.widget().expect("Controller widget").grab_focus();
                            }
                            gdk::EventType::TouchUpdate => {
                                servo.touch_update(x, y);
                            }
                            gdk::EventType::TouchEnd => {
                                servo.touch_end(x, y);
                            }
                            gdk::EventType::TouchCancel => {
                                servo.touch_cancel(x, y);
                            }
                            _ => {}
                        }
                    }
                }
                glib::Propagation::Proceed
            });
            self.obj().add_controller(legacy_controller);

            let key_controller = gtk::EventControllerKey::new();
            let obj_weak = self.obj().downgrade();
            key_controller.connect_key_pressed(move |_, keyval, _keycode, _state| {
                if let Some(obj) = obj_weak.upgrade() {
                    let imp = obj.imp();
                    if let Some(servo) = imp.servo_runner.borrow().as_ref()
                        && let Some(unicode) = keyval.to_unicode()
                    {
                        info!("Pressed key {unicode}");
                        servo.key_press(unicode);
                    }
                }
                glib::Propagation::Proceed
            });
            let obj_weak = self.obj().downgrade();
            key_controller.connect_key_released(move |_, keyval, _keycode, _state| {
                if let Some(obj) = obj_weak.upgrade() {
                    let imp = obj.imp();
                    if let Some(servo) = imp.servo_runner.borrow().as_ref()
                        && let Some(unicode) = keyval.to_unicode()
                    {
                        servo.key_release(unicode);
                    }
                }
            });
            self.obj().add_controller(key_controller);

            let scroll_controller =
                gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
            let obj_weak = self.obj().downgrade();
            scroll_controller.connect_scroll(move |_, delta_x, delta_y| {
                if let Some(obj) = obj_weak.upgrade() {
                    let imp = obj.imp();
                    if let Some(servo) = imp.servo_runner.borrow().as_ref() {
                        servo.scroll(delta_x, delta_y);
                    }
                }
                glib::Propagation::Stop
            });
            self.obj().add_controller(scroll_controller);

            self.obj().set_focusable(true);
            info!("Webview constructed");
        }

        fn dispose(&self) {
            if let Some(servo) = self.servo_runner.borrow().as_ref() {
                servo.shutdown();
            }
        }
    }

    impl WidgetImpl for WebView {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            if let Some(texture) = self.memory_texture.borrow().as_ref() {
                let bounds = gtk::graphene::Rect::new(
                    0.0,
                    0.0,
                    self.obj().width() as f32,
                    self.obj().height() as f32,
                );
                snapshot.append_texture(texture, &bounds);
            }
        }

        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            if let Some(servo) = self.servo_runner.borrow().as_ref() {
                servo.resize(width as u32, height as u32);
            }
        }
    }
}

glib::wrapper! {
    pub struct WebView(ObjectSubclass<imp::WebView>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

#[allow(clippy::new_without_default)]
impl WebView {
    pub fn new() -> Self {
        glib::Object::builder().build()
    }

    pub fn load_url(&self, url: &str) {
        let imp = self.imp();
        if let Some(servo) = imp.servo_runner.borrow().as_ref() {
            servo.load_url(url);
        }
    }

    pub fn reload(&self) {
        let imp = self.imp();
        if let Some(servo) = imp.servo_runner.borrow().as_ref() {
            servo.reload();
        }
    }

    pub fn go_back(&self) {
        let imp = self.imp();
        if let Some(servo) = imp.servo_runner.borrow().as_ref() {
            servo.go_back();
        }
    }

    pub fn go_forward(&self) {
        let imp = self.imp();
        if let Some(servo) = imp.servo_runner.borrow().as_ref() {
            servo.go_forward();
        }
    }

    fn translate_event_coordinates(&self, event: &gdk::Event) -> Option<(f64, f64)> {
        let root = self.root()?;
        let native = root.native()?;
        let (nx, ny) = native.surface_transform();

        let (event_x, event_y) = event.position()?;
        let event_x = event_x - nx;
        let event_y = event_y - ny;

        let point = gtk::graphene::Point::new(event_x as f32, event_y as f32);
        let translated = root.compute_point(self, &point)?;

        Some((translated.x() as f64, translated.y() as f64))
    }

    fn process_servo_event(&self, event: ServoEvent) {
        let Some(event_type) = event.event else {
            return;
        };

        match event_type {
            servo_event::Event::FrameReady(frame_ready) => {
                let rgba_image = RgbaImage::from_raw(
                    frame_ready.width,
                    frame_ready.height,
                    frame_ready.rgba_data,
                )
                .unwrap();

                let imp = self.imp();

                let bytes = glib::Bytes::from(&rgba_image.as_raw()[..]);
                let texture = gdk::MemoryTexture::new(
                    rgba_image.width() as i32,
                    rgba_image.height() as i32,
                    gdk::MemoryFormat::R8g8b8a8,
                    &bytes,
                    (rgba_image.width() * 4) as usize,
                );

                imp.memory_texture.replace(Some(texture));
                self.queue_draw();
            }
            servo_event::Event::CursorChanged(cursor_changed) => {
                let gdk_cursor = gdk::Cursor::from_name(&cursor_changed.cursor, None);
                if let Some(cursor) = gdk_cursor {
                    self.set_cursor(Some(&cursor));
                }
            }
            servo_event::Event::LogMessage(log_msg) => {
                if let Some(servo_runner) = self.imp().servo_runner.borrow().as_ref() {
                    servo_runner
                        .handle_log_message(LogLevel::from(log_msg.level), &log_msg.message);
                }
            }
            _ => {
                info!("Unhandled event type: {:?}", event_type);
            }
        }
    }
}
