//! Long-form Markdown guides rendered in the rustdoc item tree.
//!
//! docs.rs only shows Rust items in its sidebar, so each guide in `./docs/`
//! is exposed as an empty module whose documentation comes from the
//! corresponding Markdown file.

#[doc = include_str!("../docs/CONNECTION_DESIGN.md")]
pub mod connection_design {}

#[doc = include_str!("../docs/FEATURE_MATRIX.md")]
pub mod feature_matrix {}

#[doc = include_str!("../docs/TUTORIAL.md")]
pub mod tutorial {}

#[doc = include_str!("../docs/USAGE.md")]
pub mod usage {}
