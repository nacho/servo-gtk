/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

pub mod key_tables;
pub mod proto_ipc;
pub mod runner;
pub mod servo_runner;
pub mod web_view;

pub use web_view::WebView;

/// Hand off to the Servo runner subprocess if this process was spawned as one.
///
/// Consumers MUST call this as the very first statement in their `main()`.
/// When the process was launched as the runner subprocess, this runs the
/// runner and terminates the process without returning. Otherwise it returns
/// immediately and normal application startup can proceed.
pub fn run_as_runner_if_requested() {
    runner::run_if_requested();
}
