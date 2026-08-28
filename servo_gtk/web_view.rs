/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use crate::key_tables::KeyTables;
use crate::proto_ipc::{ServoEvent, servo_event};
use crate::servo_runner::{LogLevel, ServoRunner};
use glib::translate::*;
use glib::{info, warn};
use gtk::gdk;
use gtk::prelude::*;
use gtk::{glib, subclass::prelude::*};
use std::cell::{Cell, RefCell};

const G_LOG_DOMAIN: &str = "ServoGtk";

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct WebView {
        pub servo_runner: RefCell<Option<ServoRunner>>,
        pub memory_texture: RefCell<Option<gdk::MemoryTexture>>,
        pub key_tables: KeyTables,
        pub last_size: Cell<(i32, i32)>,
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
                    let Some(obj) = obj_weak.upgrade() else {
                        break;
                    };

                    // Take everything that queued up while the main loop was
                    // busy. Only the newest frame is ever visible, so older
                    // ones are dropped rather than decoded into textures that
                    // would be overdrawn before they reach the screen.
                    let mut batch = vec![event];
                    while let Ok(next) = event_receiver.try_recv() {
                        batch.push(next);
                    }
                    let newest_frame = batch.iter().rposition(|event| {
                        matches!(event.event, Some(servo_event::Event::FrameReady(_)))
                    });

                    for (index, event) in batch.into_iter().enumerate() {
                        let is_frame =
                            matches!(event.event, Some(servo_event::Event::FrameReady(_)));
                        if is_frame && Some(index) != newest_frame {
                            continue;
                        }
                        obj.process_servo_event(event);
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
            key_controller.connect_key_pressed(move |_, keyval, keycode, state| {
                if let Some(obj) = obj_weak.upgrade() {
                    let imp = obj.imp();
                    if let Some(servo) = imp.servo_runner.borrow().as_ref()
                        && let Some((key, is_character, location)) =
                            imp.key_tables.key_from_keyval(keyval.into_glib())
                    {
                        info!("Pressed key {key:?} at location {location:?}");
                        servo.key_press(key, is_character, location, keycode, state.bits());
                    }
                }
                glib::Propagation::Proceed
            });
            let obj_weak = self.obj().downgrade();
            key_controller.connect_key_released(move |_, keyval, keycode, state| {
                if let Some(obj) = obj_weak.upgrade() {
                    let imp = obj.imp();
                    if let Some(servo) = imp.servo_runner.borrow().as_ref()
                        && let Some((key, is_character, location)) =
                            imp.key_tables.key_from_keyval(keyval.into_glib())
                    {
                        servo.key_release(key, is_character, location, keycode, state.bits());
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
                    gtk::prelude::WidgetExt::width(self.obj().as_ref()) as f32,
                    gtk::prelude::WidgetExt::height(self.obj().as_ref()) as f32,
                );
                snapshot.append_texture(texture, &bounds);
            }
        }

        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            // GTK allocates on every layout pass. Forwarding an unchanged size
            // would cost an IPC round trip and a full Servo reflow each time.
            if self.last_size.get() == (width, height) {
                return;
            }
            self.last_size.set((width, height));

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
        let root = gtk::prelude::WidgetExt::root(self)?;
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
                let width = frame_ready.width;
                let height = frame_ready.height;
                let stride = width as usize * 4;

                // A short read on the pipe would otherwise panic here.
                if frame_ready.rgba_data.len() != stride * height as usize {
                    warn!(
                        "Discarding malformed frame: {width}x{height} with {} bytes",
                        frame_ready.rgba_data.len()
                    );
                    return;
                }

                // `Bytes::from_owned` takes the buffer over as-is. Going
                // through `&[u8]` would call `g_bytes_new`, copying the whole
                // framebuffer a second time on the main thread.
                let bytes = glib::Bytes::from_owned(frame_ready.rgba_data);
                let texture = gdk::MemoryTexture::new(
                    width as i32,
                    height as i32,
                    gdk::MemoryFormat::R8g8b8a8,
                    &bytes,
                    stride,
                );

                self.imp().memory_texture.replace(Some(texture));
                self.queue_draw();
            }
            servo_event::Event::CursorChanged(cursor_changed) => {
                let gdk_cursor = gdk::Cursor::from_name(&cursor_changed.cursor, None);
                if let Some(cursor) = gdk_cursor {
                    gtk::prelude::WidgetExt::set_cursor(self, Some(&cursor));
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
