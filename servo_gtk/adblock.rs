/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Adblock engine wrapper for the runner subprocess.
//!
//! Wraps `adblock::Engine` and provides a `check()` method that maps Servo's
//! resource destination to adblock-rust's request type. The engine runs inside
//! the runner subprocess; no IPC round-trip per request.

use adblock::Engine;
use adblock::lists::{FilterSet, ParseOptions};
use adblock::request::Request;
use std::rc::Rc;
use std::sync::atomic::{AtomicU8, Ordering};

/// Blocking level constants, mirroring `proto::AdblockLevel`.
pub const LEVEL_OFF: u8 = 0;
pub const LEVEL_STANDARD: u8 = 1;
pub const LEVEL_AGGRESSIVE: u8 = 2;

/// Adblock engine + level state.
pub struct AdblockState {
    /// Current engine, if rules have been loaded.
    ///
    /// `Rc`, not `Arc`: the engine runs entirely inside the single-threaded
    /// runner subprocess (wrapped in `Rc<RefCell<..>>` by the caller), and
    /// `adblock::Engine` is neither `Send` nor `Sync`.
    engine: Option<Rc<Engine>>,
    /// Current blocking level (0 = off, 1 = standard, 2 = aggressive).
    level: AtomicU8,
}

impl AdblockState {
    pub fn new() -> Self {
        Self {
            engine: None,
            level: AtomicU8::new(LEVEL_STANDARD),
        }
    }

    /// Load filter rules. Creates a fresh engine; any previous engine is
    /// dropped. `rules` is a slice of EasyList-format lines. If `rules` is
    /// empty, the engine is set to `None` and `check()` will always return
    /// `false`.
    pub fn load_rules(&mut self, rules: &[String]) {
        if rules.is_empty() {
            self.engine = None;
            return;
        }
        let mut filter_set = FilterSet::new(true);
        filter_set.add_filters(rules, ParseOptions::default());
        let engine = Engine::from_filter_set(filter_set, true);
        self.engine = Some(Rc::new(engine));
    }

    /// Load a pre-serialized engine blob. Replaces any existing engine.
    ///
    /// This is the fast path: the UI builds the blob once from rules,
    /// caches it on disk, and sends it here for each new tab. Deserialization
    /// takes ~200ms, versus 5-15s to parse 180k raw rules.
    ///
    /// The blob must have been produced by the same `adblock` crate version
    /// (`serialize_raw()`); a version mismatch produces a
    /// `DeserializationError::UnsupportedFormatVersion` or a decode error.
    pub fn load_blob(&mut self, blob: &[u8]) -> Result<(), String> {
        if blob.is_empty() {
            self.engine = None;
            return Ok(());
        }
        let mut engine = Engine::new(true);
        engine
            .deserialize(blob)
            .map_err(|e| format!("deserialize failed: {e:?}"))?;
        self.engine = Some(Rc::new(engine));
        Ok(())
    }

    /// Clear the engine. `check()` will always return `false` after this.
    pub fn clear(&mut self) {
        self.engine = None;
    }

    /// Set the blocking level.
    pub fn set_level(&self, level: u8) {
        self.level.store(level, Ordering::Relaxed);
    }

    /// Check whether a request should be blocked.
    ///
    /// Returns `false` if the level is `Off`, if no engine is loaded, if the
    /// request is malformed, or if the engine does not match the request.
    pub fn check(&self, url: &str, source_url: Option<&str>, resource_type: &str) -> bool {
        if self.level.load(Ordering::Relaxed) == LEVEL_OFF {
            return false;
        }
        let Some(engine) = &self.engine else {
            return false;
        };
        let source = source_url.unwrap_or("");
        // adblock-rust 0.9.x returns `Result<Request, RequestError>` from
        // `Request::new`. A malformed request is treated as "not blocked".
        let Ok(request) = Request::new(url, source, resource_type) else {
            return false;
        };
        engine.check_network_request(&request).matched
    }
}

impl Default for AdblockState {
    fn default() -> Self {
        Self::new()
    }
}

/// Map Servo's `Destination` (via its Debug format) to adblock-rust's request
/// type string. Returns `"other"` for unmapped destinations.
pub fn destination_to_resource_type(destination: &str) -> &'static str {
    match destination {
        "Document" => "document",
        "Script" => "script",
        "Image" => "image",
        "Style" => "stylesheet",
        "Font" => "font",
        "Media" | "Audio" | "Video" => "media",
        "Object" | "Embed" => "object",
        "Frame" | "Iframe" => "subdocument",
        "Xhr" => "xmlhttprequest",
        "Fetch" => "fetch",
        "WebSocket" => "websocket",
        "Ping" => "ping",
        "Beacon" => "beacon",
        "Csp" => "csp",
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_level_never_blocks() {
        let mut state = AdblockState::new();
        state.load_rules(&["||ads.example.com^".to_string()]);
        state.set_level(LEVEL_OFF);
        assert!(!state.check("https://ads.example.com/banner.png", None, "image"));
    }

    #[test]
    fn empty_rules_clears_engine() {
        let mut state = AdblockState::new();
        state.load_rules(&["||ads.example.com^".to_string()]);
        state.load_rules(&[]);
        assert!(!state.check("https://ads.example.com/banner.png", None, "image"));
    }

    #[test]
    fn clear_removes_engine() {
        let mut state = AdblockState::new();
        state.load_rules(&["||ads.example.com^".to_string()]);
        state.clear();
        assert!(!state.check("https://ads.example.com/banner.png", None, "image"));
    }

    #[test]
    fn destination_mapping() {
        assert_eq!(destination_to_resource_type("Script"), "script");
        assert_eq!(destination_to_resource_type("Image"), "image");
        assert_eq!(destination_to_resource_type("Style"), "stylesheet");
        assert_eq!(destination_to_resource_type("Document"), "document");
        assert_eq!(destination_to_resource_type("Media"), "media");
        assert_eq!(destination_to_resource_type("Manifest"), "other");
    }
}
