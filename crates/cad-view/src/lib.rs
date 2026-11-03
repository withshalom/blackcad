//! BlackCAD's viewport, without a GUI toolkit.
//!
//! The viewport of a desktop app is two jobs: knowing what to draw and
//! where, and putting it on the screen. This crate is the first job, in
//! plain Rust with the standard library only, so it builds and is tested
//! everywhere the core is:
//!
//! - [`display`]: the display list (`cad-core`'s `export::display`) decoded;
//! - [`view`]: the transform between world and screen, pan, zoom about a
//!   point, zoom to extents, the wheel step;
//! - [`grid`]: the grid and axes as screen lines;
//! - [`draw`]: the display list as screen shapes (clipped, arcs flattened,
//!   construction lines dashed, fills as triangles);
//! - [`query`]: the session's `snap` and `pick` questions and replies;
//! - [`json`]: the small JSON reader those replies need;
//! - [`viewer`]: the session, the open drawing, the view and the pointer
//!   together: all an app has to feed and paint.
//!
//! The second job, painting those shapes and reading the mouse, is
//! `apps/viewport`, which uses egui. The design (a toolkit-free layer under
//! a thin egui layer) and the algorithms for the transform, grid and
//! clipping follow CADCraft's egui canvas (MIT OR Apache-2.0; see NOTICE and
//! docs/PROVENANCE.md).

#![forbid(unsafe_code)]
// A drawing is only a claim: no input reaches a panic. Tests may unwrap.
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::unimplemented,
        clippy::todo,
        clippy::unreachable
    )
)]

pub mod display;
pub mod draw;
pub mod editor;
pub mod grid;
pub mod json;
pub mod query;
pub mod tool;
pub mod view;
pub mod viewer;

pub use display::{Bounds, Display, Item, Op, Rgb, Style};
pub use draw::{DrawOptions, Prim, Shape, tessellate, tessellate_item};
pub use editor::LayerInfo;
pub use grid::{Grid, GridLine, grid};
pub use query::{SnapHit, parse_pick, parse_snaps, pick_command, snap_command};
pub use tool::Tool;
pub use view::{View, wheel_factor};
pub use viewer::{Hover, LayerEntry, Viewer};
