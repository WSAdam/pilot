//! pilot's headful window: watch the page the agent drives, and take over
//! its input.
//!
//! The window is a second CDP client of the same browser. It follows the
//! agent's tabs through `Target.*` events, shows the active page from
//! `Page.startScreencast` frames, and forwards a human's mouse and keyboard
//! through `Input.*`. The agent owns the page's viewport; the window only
//! letterboxes the frames it is sent.

pub mod agent;
pub mod controller;
pub mod frame;
pub mod geometry;
pub mod input;
pub mod tabs;
