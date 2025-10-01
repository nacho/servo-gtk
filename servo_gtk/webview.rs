/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use crate::servo_runner::{ServoEvent, ServoRunner};
use glib::translate::*;
use glib::{info, warn};
use gtk::gdk;
use gtk::prelude::*;
use gtk::{glib, subclass::prelude::*};
use image::RgbaImage;
use std::cell::RefCell;
use std::ffi::CString;

const G_LOG_DOMAIN: &str = "ServoGtk";

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct WebView {
        pub gl_area: RefCell<Option<gtk::GLArea>>,
        pub servo_runner: RefCell<Option<ServoRunner>>,
        pub last_image: RefCell<Option<RgbaImage>>,
        pub shader_program: RefCell<u32>,
        pub vao: RefCell<u32>,
        pub texture: RefCell<u32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for WebView {
        const NAME: &'static str = "WebView";
        type Type = super::WebView;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_layout_manager_type::<gtk::BinLayout>();
        }
    }

    impl ObjectImpl for WebView {
        fn constructed(&self) {
            self.parent_constructed();

            let gl_area = gtk::GLArea::new();

            let obj_weak = self.obj().downgrade();
            gl_area.connect_realize(move |area| {
                area.make_current();
                gl::load_with(|name| epoxy::get_proc_addr(name) as *const _);

                if area.uses_es() {
                    info!("Using OpenGL ES");
                }

                if let Some(obj) = obj_weak.upgrade() {
                    let imp = obj.imp();
                    unsafe {
                        // Create shader program
                        let vertex_shader = gl::CreateShader(gl::VERTEX_SHADER);
                        let vertex_source = if area.uses_es() {
                            CString::new(
                                "#version 320 es\n\
                                 precision highp float;\n\
                                 layout (location = 0) in vec2 aPos;\n\
                                 layout (location = 1) in vec2 aTexCoord;\n\
                                 out vec2 TexCoord;\n\
                                 void main() {\n\
                                     gl_Position = vec4(aPos, 0.0, 1.0);\n\
                                     TexCoord = aTexCoord;\n\
                                 }",
                            )
                            .expect("Vertex source")
                        } else {
                            CString::new(
                                "#version 330 core\n\
                                 layout (location = 0) in vec2 aPos;\n\
                                 layout (location = 1) in vec2 aTexCoord;\n\
                                 out vec2 TexCoord;\n\
                                 void main() {\n\
                                     gl_Position = vec4(aPos, 0.0, 1.0);\n\
                                     TexCoord = aTexCoord;\n\
                                 }",
                            )
                            .expect("Vertex source")
                        };

                        gl::ShaderSource(
                            vertex_shader,
                            1,
                            &vertex_source.as_ptr(),
                            std::ptr::null(),
                        );
                        gl::CompileShader(vertex_shader);
                        let mut status: i32 = 0;
                        gl::GetShaderiv(vertex_shader, gl::COMPILE_STATUS, &mut status);
                        if status == 0 {
                            let mut buf: [u8; 1024] = [0; 1024];
                            gl::GetShaderInfoLog(
                                vertex_shader,
                                1024,
                                std::ptr::null_mut(),
                                buf.as_mut_ptr().cast(),
                            );
                            warn!("INFO LOG {:?}", std::str::from_utf8(&buf[..]).unwrap());
                        }

                        let fragment_shader = gl::CreateShader(gl::FRAGMENT_SHADER);
                        let fragment_source = if area.uses_es() {
                            CString::new(
                                "#version 320 es\n\
                                 precision highp float;\n\
                                 out vec4 FragColor;\n\
                                 in vec2 TexCoord;\n\
                                 uniform sampler2D ourTexture;\n\
                                 void main() {\n\
                                     FragColor = texture(ourTexture, TexCoord);\n\
                                 }",
                            )
                            .expect("Fragment source")
                        } else {
                            CString::new(
                                "#version 330 core\n\
                                 out vec4 FragColor;\n\
                                 in vec2 TexCoord;\n\
                                 uniform sampler2D ourTexture;\n\
                                 void main() {\n\
                                     FragColor = texture(ourTexture, TexCoord);\n\
                                 }",
                            )
                            .expect("Fragment source")
                        };

                        gl::ShaderSource(
                            fragment_shader,
                            1,
                            &fragment_source.as_ptr(),
                            std::ptr::null(),
                        );
                        gl::CompileShader(fragment_shader);
                        let mut status: i32 = 0;
                        gl::GetShaderiv(fragment_shader, gl::COMPILE_STATUS, &mut status);
                        if status == 0 {
                            let mut buf: [u8; 1024] = [0; 1024];
                            gl::GetShaderInfoLog(
                                fragment_shader,
                                1024,
                                std::ptr::null_mut(),
                                buf.as_mut_ptr().cast(),
                            );
                            warn!("INFO LOG {:?}", std::str::from_utf8(&buf[..]).unwrap());
                        }

                        let program = gl::CreateProgram();
                        gl::AttachShader(program, vertex_shader);
                        gl::AttachShader(program, fragment_shader);
                        gl::LinkProgram(program);
                        gl::DeleteShader(vertex_shader);
                        gl::DeleteShader(fragment_shader);

                        imp.shader_program.replace(program);

                        // Create VAO and VBO
                        let vertices: [f32; 16] = [
                            -1.0, -1.0, 0.0, 1.0, // bottom left
                            1.0, -1.0, 1.0, 1.0, // bottom right
                            1.0, 1.0, 1.0, 0.0, // top right
                            -1.0, 1.0, 0.0, 0.0, // top left
                        ];
                        let indices: [u32; 6] = [0, 1, 2, 2, 3, 0];

                        let mut vao = 0;
                        let mut vbo = 0;
                        let mut ebo = 0;
                        gl::GenVertexArrays(1, &mut vao);
                        gl::GenBuffers(1, &mut vbo);
                        gl::GenBuffers(1, &mut ebo);

                        gl::BindVertexArray(vao);
                        gl::BindBuffer(gl::ARRAY_BUFFER, vbo);
                        gl::BufferData(
                            gl::ARRAY_BUFFER,
                            (vertices.len() * std::mem::size_of::<f32>()) as isize,
                            vertices.as_ptr() as *const _,
                            gl::STATIC_DRAW,
                        );

                        gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, ebo);
                        gl::BufferData(
                            gl::ELEMENT_ARRAY_BUFFER,
                            (indices.len() * std::mem::size_of::<u32>()) as isize,
                            indices.as_ptr() as *const _,
                            gl::STATIC_DRAW,
                        );

                        gl::VertexAttribPointer(
                            0,
                            2,
                            gl::FLOAT,
                            gl::FALSE,
                            4 * std::mem::size_of::<f32>() as i32,
                            std::ptr::null(),
                        );
                        gl::EnableVertexAttribArray(0);
                        gl::VertexAttribPointer(
                            1,
                            2,
                            gl::FLOAT,
                            gl::FALSE,
                            4 * std::mem::size_of::<f32>() as i32,
                            (2 * std::mem::size_of::<f32>()) as *const _,
                        );
                        gl::EnableVertexAttribArray(1);

                        imp.vao.replace(vao);

                        // Create texture
                        let mut texture = 0;
                        gl::GenTextures(1, &mut texture);
                        imp.texture.replace(texture);
                    }
                }
            });

            let obj_weak = self.obj().downgrade();
            gl_area.connect_render(move |_, _| {
                if let Some(obj) = obj_weak.upgrade() {
                    let imp = obj.imp();
                    if let Some(rgba_image) = imp.last_image.borrow().as_ref() {
                        unsafe {
                            gl::Clear(gl::COLOR_BUFFER_BIT);

                            // Update texture
                            gl::BindTexture(gl::TEXTURE_2D, *imp.texture.borrow());
                            gl::TexImage2D(
                                gl::TEXTURE_2D,
                                0,
                                gl::RGBA as i32,
                                rgba_image.width() as i32,
                                rgba_image.height() as i32,
                                0,
                                gl::RGBA,
                                gl::UNSIGNED_BYTE,
                                rgba_image.as_raw().as_ptr() as *const _,
                            );
                            gl::TexParameteri(
                                gl::TEXTURE_2D,
                                gl::TEXTURE_MIN_FILTER,
                                gl::LINEAR as i32,
                            );
                            gl::TexParameteri(
                                gl::TEXTURE_2D,
                                gl::TEXTURE_MAG_FILTER,
                                gl::LINEAR as i32,
                            );

                            // Render
                            gl::UseProgram(*imp.shader_program.borrow());
                            gl::BindVertexArray(*imp.vao.borrow());
                            gl::DrawElements(gl::TRIANGLES, 6, gl::UNSIGNED_INT, std::ptr::null());
                        }
                    }
                }
                glib::Propagation::Stop
            });

            let obj_weak = self.obj().downgrade();
            gl_area.connect_resize(move |area, _width, _height| {
                if let Some(obj) = obj_weak.upgrade() {
                    let imp = obj.imp();

                    if let Some(servo) = imp.servo_runner.borrow().as_ref() {
                        servo.resize(area.width() as u32, area.height() as u32);
                    }
                }
            });

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
            gl_area.add_controller(motion_controller);

            let legacy_controller = gtk::EventControllerLegacy::new();
            let obj_weak = self.obj().downgrade();
            legacy_controller.connect_event(move |_, event| {
                if let Some(obj) = obj_weak.upgrade() {
                    let imp = obj.imp();
                    if let Some(servo) = imp.servo_runner.borrow().as_ref() {
                        if let Some((x, y)) = obj.translate_event_coordinates(event) {
                            match event.event_type() {
                                gdk::EventType::ButtonPress => {
                                    if let Some(button_event) =
                                        event.downcast_ref::<gdk::ButtonEvent>()
                                    {
                                        servo.button_press(button_event.button(), x, y);
                                    }
                                }
                                gdk::EventType::ButtonRelease => {
                                    if let Some(button_event) =
                                        event.downcast_ref::<gdk::ButtonEvent>()
                                    {
                                        servo.button_release(button_event.button(), x, y);
                                    }
                                }
                                gdk::EventType::TouchBegin => {
                                    servo.touch_begin(x, y);
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
                }
                glib::Propagation::Proceed
            });
            gl_area.add_controller(legacy_controller);

            let key_controller = gtk::EventControllerKey::new();
            let obj_weak = self.obj().downgrade();
            key_controller.connect_key_pressed(move |_, keyval, _keycode, _state| {
                if let Some(obj) = obj_weak.upgrade() {
                    let imp = obj.imp();
                    if let Some(servo) = imp.servo_runner.borrow().as_ref() {
                        if let Some(unicode) = keyval.to_unicode() {
                            servo.key_press(unicode);
                        }
                    }
                }
                glib::Propagation::Proceed
            });
            let obj_weak = self.obj().downgrade();
            key_controller.connect_key_released(move |_, keyval, _keycode, _state| {
                if let Some(obj) = obj_weak.upgrade() {
                    let imp = obj.imp();
                    if let Some(servo) = imp.servo_runner.borrow().as_ref() {
                        if let Some(unicode) = keyval.to_unicode() {
                            servo.key_release(unicode);
                        }
                    }
                }
            });
            gl_area.add_controller(key_controller);

            // FIXME: ideally we would do some proper size measuring so
            // we can embed the webview in a scrolled window. Checking the api
            // we do not seem to get any notification of the page size so
            // I don't think this can be done for now
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
            gl_area.add_controller(scroll_controller);

            gl_area.set_parent(&*self.obj());
            self.gl_area.replace(Some(gl_area));

            let servo_runner = ServoRunner::new();
            let event_receiver = servo_runner.event_receiver().clone();
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

            self.servo_runner.replace(Some(servo_runner));

            info!("Webview constructed");
        }

        fn dispose(&self) {
            if let Some(servo) = self.servo_runner.borrow().as_ref() {
                servo.shutdown();
            }
        }
    }

    impl WidgetImpl for WebView {}
}

glib::wrapper! {
    pub struct WebView(ObjectSubclass<imp::WebView>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

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

    fn translate_event_coordinates(&self, event: &gdk::Event) -> Option<(f64, f64)> {
        let root = self.root()?;
        let native = root.native()?;
        let (nx, ny) = native.surface_transform();

        let (event_x, event_y) = event.position()?;
        let event_x = event_x - nx;
        let event_y = event_y - ny;

        let gl_area = self.imp().gl_area.borrow();
        let gl_area = gl_area.as_ref()?;

        let point = gtk::graphene::Point::new(event_x as f32, event_y as f32);
        let translated = root.compute_point(gl_area, &point)?;

        Some((translated.x() as f64, translated.y() as f64))
    }

    fn process_servo_event(&self, event: ServoEvent) {
        match event {
            // FIXME: this is just a hack to get me going. Ideally we would
            // use a DMA-Buf so we avoid movign the pixels from the GPU to
            // system memory and back to the GPU
            ServoEvent::FrameReady(rgba_image) => {
                let imp = self.imp();

                imp.last_image.replace(Some(rgba_image));

                if let Some(gl_area) = imp.gl_area.borrow().as_ref() {
                    gl_area.queue_draw();
                }
            }
            ServoEvent::LoadComplete => {
                info!("Page load complete");
            }
            ServoEvent::CursorChanged(cursor) => {
                let gdk_cursor = match cursor {
                    servo::Cursor::Default => gdk::Cursor::from_name("default", None),
                    servo::Cursor::Pointer => gdk::Cursor::from_name("pointer", None),
                    servo::Cursor::Text => gdk::Cursor::from_name("text", None),
                    servo::Cursor::Wait => gdk::Cursor::from_name("wait", None),
                    servo::Cursor::Help => gdk::Cursor::from_name("help", None),
                    servo::Cursor::Crosshair => gdk::Cursor::from_name("crosshair", None),
                    servo::Cursor::Move => gdk::Cursor::from_name("move", None),
                    servo::Cursor::NotAllowed => gdk::Cursor::from_name("not-allowed", None),
                    servo::Cursor::Grab => gdk::Cursor::from_name("grab", None),
                    servo::Cursor::Grabbing => gdk::Cursor::from_name("grabbing", None),
                    _ => gdk::Cursor::from_name("default", None),
                };

                if let Some(cursor) = gdk_cursor {
                    self.set_cursor(Some(&cursor));
                }
            }
        }
    }
}
