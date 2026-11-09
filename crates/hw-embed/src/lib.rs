//! Nested Wayland compositor (smithay) that runs other applications inside a
//! hyprwidget layer surface as an ordinary widget (`W.App`).
//!
//! See `PLAN.md` §6, `docs/embed-spike.md` (M0.5) and `docs/embed.md` (M5).

mod buffer;
mod clipboard;
mod input;
mod popup;
mod process;
mod server;
mod surface;
mod text_input;
mod view;

pub use server::EmbedServer;
pub use view::{EmbedView, Mode, Restart};
