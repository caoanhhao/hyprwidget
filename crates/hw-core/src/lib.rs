//! Core building blocks shared by every other crate: config directory and
//! `config.json`, the reactive model (Signal/computed/poll), the AGS-style
//! settings store, the IPC socket, Hyprland option inheritance and color
//! parsing.

pub mod color;
pub mod config;
pub mod hyprland;
pub mod ipc;
pub mod options;
pub mod reactive;
pub mod settings;

pub use reactive::{Signal, Subscription, computed, poll};
