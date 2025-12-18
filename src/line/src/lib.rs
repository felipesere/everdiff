use std::{
    fmt::{self},
    num::NonZeroUsize,
    ops::{Add, Sub},
};

#[derive(PartialEq, Eq, PartialOrd, Ord, Copy, Clone)]
pub struct Line(NonZeroUsize);

impl fmt::Debug for Line {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("Line({})", &self.0))
    }
}

impl Line {
    pub fn get(&self) -> usize {
        self.0.get()
    }

    pub fn new(raw: usize) -> Option<Self> {
        Some(Line(NonZeroUsize::try_from(raw).ok()?))
    }

    /// Create a Line without checking if the value is valid.
    /// This will panic if n is 0.
    pub fn unchecked(n: usize) -> Self {
        Self(NonZeroUsize::try_from(n).unwrap())
    }

    pub fn one() -> Self {
        Self::new(1).unwrap()
    }

    pub fn distance(&self, other: &Line) -> usize {
        let a = self.get();
        let b = other.get();

        a.abs_diff(b)
    }

    /// Subtract with saturation - returns Line(1) if the result would be zero or negative.
    pub fn saturating_sub(self, rhs: usize) -> Line {
        let val = self.0.get();
        if val <= rhs {
            Line::one()
        } else {
            Line::new(val - rhs).unwrap()
        }
    }
}

impl fmt::Display for Line {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Add<usize> for Line {
    type Output = Line;

    fn add(self, rhs: usize) -> Self::Output {
        Line(self.0.saturating_add(rhs))
    }
}

impl Sub<usize> for Line {
    type Output = Option<Line>;

    fn sub(self, rhs: usize) -> Self::Output {
        let val = self.0.get();
        if val <= rhs {
            None
        } else {
            Line::new(val - rhs)
        }
    }
}

impl PartialOrd<usize> for Line {
    fn partial_cmp(&self, other: &usize) -> Option<std::cmp::Ordering> {
        self.0.get().partial_cmp(other)
    }
}

impl PartialEq<usize> for Line {
    fn eq(&self, other: &usize) -> bool {
        self.0.get().eq(other)
    }
}
