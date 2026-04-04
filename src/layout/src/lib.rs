//! Side-by-side terminal layout for everdiff diffs.
//!
//! This crate turns styled YAML content into two parallel columns of fixed-width
//! terminal output — the visual backbone of everdiff's diff view.
//!
//! # Rendering pipeline
//!
//! ```text
//!                        PrefixedLine
//!                       (adds chrome)
//!                            │
//! StyledContent ─────────────┤
//! (wrap + style → Vec<String>│         Column ──► ColumnPair::zip ──► Vec<String>
//!                            │        /
//! Lineable ──────────────────┴──► LineGroup
//! (wrap → LineGroup, no chrome)   (Vec<FormattedRow>)
//! ```
//!
//! 1. **[`StyledContent`]** — borrows `&self` and produces `Vec<String>`: plain text
//!    wrapped to a fixed width with ANSI colour codes applied per segment so they
//!    never straddle a line boundary. Implemented by [`Highlighted`] (uniform colour)
//!    and [`InlineParts`] (per-span colours for word-wise diffs). Used internally by
//!    [`PrefixedLine`] to obtain styled strings before framing them with chrome.
//! 2. **[`Lineable`]** — consumes `self` and produces a [`LineGroup`]
//!    (`Vec<`[`FormattedRow`]`>`): the column-ready representation. Wraps long text
//!    into multiple rows but adds no chrome itself. [`PrefixedLine`] is the one
//!    implementor that *does* add chrome — it calls [`StyledContent::styled_segments`]
//!    on its inner content and then frames each segment with `│ nr │`. Plain `String`
//!    and `&str` implement [`Lineable`] directly for unstyled header and label rows.
//! 3. **[`Column`]** — one side of the two-column layout. Push [`Lineable`] values in
//!    order; each becomes a [`LineGroup`] (one or more [`FormattedRow`]s when a line
//!    wraps).
//! 4. **[`ColumnPair`]** — owns the terminal-width-to-column-width conversion. Create
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
pub use content::{Highlight, Highlighted, InlineParts, StyledContent};
