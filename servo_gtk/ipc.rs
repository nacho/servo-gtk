use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServoEvent {
    FrameReady(Vec<u8>, u32, u32), // Raw RGBA data, width, height
    LoadComplete,
    CursorChanged(String), // Cursor type as string
}
