//! Nested Wayland compositor (smithay) that runs other applications inside a
//! hyprwidget layer surface as an ordinary widget (`W.App`).
//!
//! Spike in M0.5, complete in M5 (see `PLAN.md` §6 and `docs/embed-spike.md`).

mod server;
mod view;

pub use server::EmbedServer;
pub use view::EmbedView;
