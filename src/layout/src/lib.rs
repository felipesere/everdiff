//! Side-by-side terminal layout for everdiff diffs.
//!
//! This crate helps shape content into to two parallel columns of fixed-width
//! terminal output — the visual backbone of everdiff's diff view.
//!
//! # Rendering pipeline
//!
//! ```text
//! Lineable ──► LineGroup ──► Column ──► ColumnPair::zip ──► Vec<String>
//! ```
//!
//! 1. **[`Lineable`]** — renders `&self` into a [`LineGroup`]
//!    (`Vec<`[`FormattedRow`]`>`), wrapping text that exceeds the column width into
//!    multiple rows. Implemented by plain `String`/`&str` (unstyled headers and
//!    labels), [`Highlighted`] (uniform ANSI colour), [`InlineParts`] (per-span
//!    colours for word-wise diffs), and [`PrefixedLine`] (decorates another
//!    [`Lineable`] with a line-number prefix).
//! 2. **[`Column`]** — one side of the two-column layout. Push [`Lineable`] values in
//!    order; each becomes a [`LineGroup`] (one or more [`FormattedRow`]s when a line
//!    wraps).
//! 3. **[`ColumnPair`]** — owns the terminal-width-to-column-width conversion. Create
//!    both columns from it, fill them, then call [`ColumnPair::zip`] to interleave
//!    their rows into a `Vec<String>` ready for printing.
//!
//! # Typical usage
//!
//! ```rust,ignore
//! let pair  = ColumnPair::new(terminal_width);
//! let mut left  = pair.column();
//! let mut right = pair.column();
//!
//! left.push(PrefixedLine::numbered(0, Highlighted::new("key: old", dimmed.clone())));
//! right.push(PrefixedLine::numbered(0, Highlighted::new("key: new", changed.clone())));
//!
//! for line in pair.zip(left, right) {
//!     println!("{line}");
//! }
//! ```

mod column;
pub mod content;
mod wrap;

pub use column::{Column, ColumnPair, FormattedRow, LineGroup, Lineable, PrefixedLine};
pub use content::{Highlight, Highlighted, InlineParts};
