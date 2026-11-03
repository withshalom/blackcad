//! The viewport's geometry for a native canvas.
//!
//! BlackCAD's core renders a drawing into a *display list* (`DSPL`, see
//! `cad_core::export::display`): lines, true arcs, polylines, even-odd fills,
//! dots and construction lines, with a table of styles. A canvas has to turn
//! that into something a GPU draws. This crate does that, and keeps the view
//! (zoom, pan, fit) and the pick test, with no windowing or GPU code, so it
//! runs and is tested the same everywhere. The desktop app uploads the
//! buffers it makes; the byte layout of [`Mesh`] is the vertex layout of the
//! app's shader.
//!
//! - [`Display::parse`] reads the bytes (and [`Display::apply_patch`] applies
//!   a patch, so a change to one entity does not re-read the drawing).
//! - [`Mesh::build`] tessellates for a scale: arcs get the segments that
//!   keep them within a fraction of a pixel, fills are triangulated, and
//!   positions are `f32` relative to an origin near the view so large world
//!   coordinates keep their precision.
//! - [`View`] maps world and screen and zooms about the cursor.
//! - [`Display::pick`] finds the entity under a point.

mod earcut;
pub mod json;
mod mesh;
mod parse;
mod pick;
mod view;

pub use mesh::{arc_points, construction_lines, Mesh, MeshStats, Options, Segment, INSTANCE_SIZE, VERTEX_SIZE};
pub use parse::{Display, Item, Op, Style};
pub use view::View;

/// Why a display list could not be read.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// Not a display list: wrong magic or layout version.
    NotADisplayList(String),
    /// The JSON header is damaged.
    Header(String),
    /// The operations end inside an operation, or name something that is not there.
    Format(String),
    /// A mesh would be larger than a canvas can hold.
    TooLarge,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotADisplayList(m) => write!(f, "not a display list: {m}"),
            Error::Header(m) => write!(f, "display list header: {m}"),
            Error::Format(m) => write!(f, "display list: {m}"),
            Error::TooLarge => write!(f, "the drawing is too large to draw at this size"),
        }
    }
}

impl std::error::Error for Error {}
