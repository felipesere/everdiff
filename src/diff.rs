use std::collections::HashSet;

#[derive(Clone, Debug, Eq, PartialEq)]
use crate::path::{Path, Segment};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Difference {
    Added {
        path: Path,
        value: serde_yaml::Value,
    },
    Removed {
        path: Path,
        value: serde_yaml::Value,
    },
    Changed {
        path: Path,
        left: serde_yaml::Value,
        right: serde_yaml::Value,
    },
    Moved {
        original_path: Path,
        new_path: Path,
    },
}

impl Difference {
    pub fn path(&self) -> Path {
        match self {
            Difference::Added { path, .. } => path.clone(),
            Difference::Removed { path, .. } => path.clone(),
            Difference::Changed { path, .. } => path.clone(),
            Difference::Moved { original_path, .. } => original_path,
        }
    }
}

#[derive(Default, Clone, Debug, Eq, PartialEq)]
pub struct Path(Vec<Segment>);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Segment {
    Field(serde_yaml::Value),
    Index(usize),
}

impl From<&str> for Segment {
    fn from(value: &str) -> Self {
        Segment::Field(value.into())
    }
}

impl From<serde_yaml::Value> for Segment {
    fn from(val: serde_yaml::Value) -> Self {
        Segment::Field(val)
    }
}

impl From<usize> for Segment {
    fn from(val: usize) -> Self {
        Segment::Index(val)
    }
}

impl Path {
    pub fn jq_like(&self) -> String {
        let mut buf = String::new();
        for s in &self.0 {
            match s {
                Segment::Field(serde_yaml::Value::String(s)) => {
                    buf += &format!(".{s}");
                }
                Segment::Field(other) => panic!("{other:?} not supported for jq_like"),
                Segment::Index(n) => {
                    buf += &format!("[{n}]");
                }
            };
        }
        buf
    }

    pub fn push(&self, value: impl Into<Segment>) -> Self {
        let mut copy = self.clone();
        copy.0.push(value.into());
        copy
    }

    #[cfg(test)]
    pub fn from_unchecked(path: Vec<Segment>) -> Self {
        Path(path)
    }
}

impl Difference {
    pub fn path(&self) -> &Path {
        match self {
            Difference::Added { path, .. } => path,
            Difference::Removed { path, .. } => path,
            Difference::Changed { path, .. } => path,
            Difference::Moved { original_path, .. } => original_path,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrayOrdering {
    Fixed,
    Dynamic,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Context {
    path: Path,
    pub array_ordering: ArrayOrdering,
}

impl Default for Context {
    fn default() -> Self {
        Self {
            path: Path::default(),
            array_ordering: ArrayOrdering::Fixed,
        }
    }
}

impl Context {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn for_key(&self, key: impl Into<Segment>) -> Context {
        let mut copy = self.clone();
        copy.path = self.path.push(key);
        copy
    }
}

/// Under a given context `ctx`, extract the differneces between `left` and `right`
pub fn diff(ctx: Context, left: &serde_yaml::Value, right: &serde_yaml::Value) -> Vec<Difference> {
    use serde_yaml::Value;

    match (left, right) {
        (Value::Mapping(left), Value::Mapping(right)) => {
            let left_keys: HashSet<_> = left.keys().collect();
            let right_keys: HashSet<_> = right.keys().collect();

            let all_keys: HashSet<_> = left_keys.union(&right_keys).collect();
            let mut diffs = Vec::new();
            for key in all_keys {
                let path = ctx.path.push((*key).clone());
                match (left.get(key), right.get(key)) {
                    (None, None) => unreachable!("the key must be from either left or right!"),
                    (None, Some(addition)) => diffs.push(Difference::Added {
                        path,
                        value: addition.clone(),
                    }),
                    (Some(removal), None) => diffs.push(Difference::Removed {
                        path,
                        value: removal.clone(),
                    }),
                    (Some(left), Some(right)) => {
                        diffs.append(&mut diff(ctx.for_key((*key).clone()), left, right));
                    }
                }
            }
            diffs
        }
        (Value::Sequence(left_elements), Value::Sequence(right_elements)) => {
            if ctx.array_ordering == ArrayOrdering::Fixed {
                // we start by comparing the in order
                let max_element_idx = std::cmp::max(left_elements.len(), right_elements.len());
                let mut diffs = Vec::new();
                for idx in 0..max_element_idx {
                    match (left_elements.get(idx), right_elements.get(idx)) {
                        (None, None) => {
                            unreachable!("the index must be from either left or right!")
                        }
                        (None, Some(addition)) => diffs.push(Difference::Added {
                            path: ctx.path.push(idx),
                            value: addition.clone(),
                        }),
                        (Some(removal), None) => diffs.push(Difference::Removed {
                            path: ctx.path.push(idx),
                            value: removal.clone(),
                        }),
                        (Some(left), Some(right)) => {
                            diffs.append(&mut diff(ctx.for_key(idx), left, right));
                        }
                    }
                }
                diffs
            } else {
                let mut difference_matrix =
                    vec![vec![Vec::<Difference>::new(); right_elements.len()]; left_elements.len()];

                for (ldx, left_value) in left_elements.iter().enumerate() {
                    for (rdx, right_value) in right_elements.iter().enumerate() {
                        difference_matrix[ldx][rdx] =
                            diff(ctx.for_key(ldx), left_value, right_value);
                    }
                }

                let MatchingOutcome {
                    added,
                    removed,
                    changed,
                    moved,
                } = minimize_differences(&difference_matrix);

                let mut diffs = Vec::new();
                for idx in removed {
                    diffs.push(Difference::Removed {
                        path: ctx.path.push(idx),
                        value: left_elements[idx].clone(),
                    });
                }

                for idx in added {
                    diffs.push(Difference::Added {
                        path: ctx.path.push(idx),
                        value: right_elements[idx].clone(),
                    });
                }

                for (ldx, rdx) in moved {
                    diffs.push(Difference::Moved {
                        original_path: ctx.path.push(ldx),
                        new_path: ctx.path.push(rdx),
                    });
                }

                diffs.append(&mut changed.into_iter().flat_map(|(_, _, diff)| diff).collect());
                diffs
            }
        }
        // if the values are the same, no need to further diff
        (left, right) if left == right => Vec::new(),
        (left, right) => {
            vec![Difference::Changed {
                path: ctx.path.clone(),
                left: left.clone(),
                right: right.clone(),
            }]
        }
    }
}

type DiffMatrix = Vec<Vec<Vec<Difference>>>;

struct MatchingOutcome {
    added: Vec<usize>,
    removed: Vec<usize>,
    moved: Vec<(usize, usize)>,
    changed: Vec<(usize, usize, Vec<Difference>)>,
}

/// Take in a matrix of differneces and produce a set of indizes that minimize it
fn minimize_differences(matrix: &DiffMatrix) -> MatchingOutcome {
    let mut changed: Vec<(usize, usize, Vec<Difference>)> = Vec::new();
    let mut moved: Vec<(usize, usize)> = Vec::new();
    // this is getting stupid... I need to track these better...
    let mut unmoved: Vec<usize> = Vec::new();

    let mut used_right_indexes = Vec::new();
    let mut used_left_indexes = Vec::new();

    'outer: for (ldx, right_values) in matrix.iter().enumerate() {
        let mut right_idx_and_diff: Vec<_> = right_values.iter().enumerate().collect();
        // Sort by amount of differences, most similar (0 difference) to the most different
        right_idx_and_diff.sort_by_key(|(_, diff)| diff.len());

        for (rdx, diffs) in right_idx_and_diff {
            // Pick the least different index that has not been used yet
            if !used_right_indexes.contains(&rdx) {
                if diffs.is_empty() {
                    if ldx == rdx {
                        unmoved.push(ldx);
                    } else {
                        moved.push((ldx, rdx));
                    }
                    used_left_indexes.push(ldx);
                    used_right_indexes.push(rdx);
                } else {
                    changed.push((ldx, rdx, diffs.clone()));
                    used_right_indexes.push(rdx);
                    used_left_indexes.push(ldx);
                }
                // found a match, so we can move on!
                continue 'outer;
            }
        }
    }
    // removed and added indexes are the ones that are neither changed nor morved
    let removed_indexes: Vec<_> = (0..matrix.len())
        .filter(|ldx| !used_left_indexes.contains(ldx))
        .collect();

    let len = matrix.first().map_or(0, |m| m.len());
    let added_indexes: Vec<_> = (0..len)
        .filter(|rdx| !used_right_indexes.contains(rdx))
        .collect();

    MatchingOutcome {
        added: added_indexes,
        removed: removed_indexes,
        moved,
        changed,
    }
}

#[cfg(test)]
mod tests {
    use expect_test::expect;
    use indoc::indoc;
    use pretty_assertions::assert_eq;

    use crate::diff::ArrayOrdering;

    use super::{diff, Context, Difference, Path};

    #[test]
    fn simple_values_changes() {
        let left = serde_yaml::from_str(indoc! {r#"
        foo:
          bar: 1
        "#})
        .unwrap();

        let right = serde_yaml::from_str(indoc! {r#"
        foo:
          bar: 2
        "#})
        .unwrap();

        let differences = diff(Context::new(), &left, &right);

        assert_eq!(
            differences,
            vec![Difference::Changed {
                left: serde_yaml::Value::Number(1.into()),
                right: serde_yaml::Value::Number(2.into()),
                path: Path::from_unchecked(vec!["foo".into(), "bar".into(),])
            }]
        )
    }

    #[test]
    fn added_or_changed_element_in_array() {
        let left = serde_yaml::from_str(indoc! {r#"
        foo:
          - a
          - b
          - c
        "#})
        .unwrap();

        let right = serde_yaml::from_str(indoc! {r#"
        foo:
          - x
          - b
          - c
          - d
        "#})
        .unwrap();

        let differences = diff(Context::new(), &left, &right);

        assert_eq!(
            differences,
            vec![
                Difference::Changed {
                    left: serde_yaml::Value::String("a".into()),
                    right: serde_yaml::Value::String("x".into()),
                    path: Path::from_unchecked(vec!["foo".into(), 0.into(),])
                },
                Difference::Added {
                    path: Path::from_unchecked(vec!["foo".into(), 3.into()]),
                    value: serde_yaml::Value::String("d".to_string())
                }
            ]
        )
    }

    #[test]
    fn removed_element_in_vector() {
        let left = serde_yaml::from_str(indoc! {r#"
        foo:
          - a
          - b
          - c
        "#})
        .unwrap();

        let right = serde_yaml::from_str(indoc! {r#"
        foo:
          - a
          - b
        "#})
        .unwrap();

        let differences = diff(Context::new(), &left, &right);

        assert_eq!(
            differences,
            vec![Difference::Removed {
                path: Path::from_unchecked(vec!["foo".into(), 2.into()]),
                value: serde_yaml::Value::String("c".to_string())
            }]
        )
    }

    #[test]
    fn type_change() {
        let left = serde_yaml::from_str(indoc! {r#"
        foo:
          bar: "12"
        "#})
        .unwrap();

        let right = serde_yaml::from_str(indoc! {r#"
        foo:
          bar: false
        "#})
        .unwrap();

        let differences = diff(Context::new(), &left, &right);

        assert_eq!(
            differences,
            vec![Difference::Changed {
                left: serde_yaml::Value::String("12".into()),
                right: serde_yaml::Value::Bool(false),
                path: Path::from_unchecked(vec!["foo".into(), "bar".into(),])
            },]
        )
    }

    #[test]
    fn netpol_example() {
        let left = serde_yaml::from_str(indoc! {r#"
        egress:
          - ports:
            - port: 80
              protocol: TCP
            to:
            - ipBlock:
                cidr: 169.254.169.254/32
        "#})
        .unwrap();

        let right = serde_yaml::from_str(indoc! {r#"
        egress:
          - to:
            - ipBlock:
                cidr: 169.254.169.254/32
            ports:
            - port: 80
        "#})
        .unwrap();

        let differences = diff(Context::new(), &left, &right);

        assert_eq!(
            differences,
            vec![Difference::Removed {
                path: Path::from_unchecked(vec![
                    "egress".into(),
                    0.into(),
                    "ports".into(),
                    0.into(),
                    "protocol".into()
                ]),
                value: serde_yaml::Value::String("TCP".into()),
            }]
        )
    }

    #[test]
    fn reordered_array_should_still_be_equal() {
        let left = serde_yaml::from_str(indoc! {r#"
        some_list:
          - name: alpha
            value:
              wheels: 1
              doors: 1
          - name: bravo
            value:
              wheels: 2
              doors: 2
          - name: charlie
            value:
              wheels: 3
              doors: 3
        "#})
        .unwrap();

        let right = serde_yaml::from_str(indoc! {r#"
        some_list:
          - name: bravo
            value:
              wheels: 2
              doors: 2
          - name: lambda
            value:
              wheels: 9
              doors: 9
          - name: charlie
            value:
              wheels: 3
              doors: 3
          - name: alpha
            value:
              wheels: 1
              doors: 2 # <--- slightly changed!
        "#})
        .unwrap();

        let mut ctx = Context::new();
        ctx.array_ordering = ArrayOrdering::Dynamic;

        let differences = diff(ctx, &left, &right);
        expect![[r#"
            [
                Added {
                    path: Path(
                        [
                            Field(
                                String("some_list"),
                            ),
                            Index(
                                1,
                            ),
                        ],
                    ),
                    value: Mapping {
                        "name": String("lambda"),
                        "value": Mapping {
                            "wheels": Number(9),
                            "doors": Number(9),
                        },
                    },
                },
                Moved {
                    original_path: Path(
                        [
                            Field(
                                String("some_list"),
                            ),
                            Index(
                                1,
                            ),
                        ],
                    ),
                    new_path: Path(
                        [
                            Field(
                                String("some_list"),
                            ),
                            Index(
                                0,
                            ),
                        ],
                    ),
                },
                Changed {
                    path: Path(
                        [
                            Field(
                                String("some_list"),
                            ),
                            Index(
                                0,
                            ),
                            Field(
                                String("value"),
                            ),
                            Field(
                                String("doors"),
                            ),
                        ],
                    ),
                    left: Number(1),
                    right: Number(2),
                },
            ]
        "#]]
        .assert_debug_eq(&differences);
    }
}
