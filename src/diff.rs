use std::collections::HashSet;

use saphyr::YamlDataOwned;

use crate::path::{Path, Segment};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Difference {
    Added {
        path: Path,
        value: saphyr::MarkedYamlOwned,
    },
    Removed {
        path: Path,
        value: saphyr::MarkedYamlOwned,
    },
    Changed {
        path: Path,
        left: saphyr::MarkedYamlOwned,
        right: saphyr::MarkedYamlOwned,
    },
    Moved {
        original_path: Path,
        new_path: Path,
    },
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

/// Under a given context `ctx`, extract the differences between `left` and `right`
pub fn diff(
    ctx: Context,
    left: &saphyr::MarkedYamlOwned,
    right: &saphyr::MarkedYamlOwned,
) -> Vec<Difference> {
    match (&left.data, &right.data) {
        (YamlDataOwned::Mapping(left), YamlDataOwned::Mapping(right)) => {
            let left_keys: HashSet<_> = left.keys().collect();
            let right_keys: HashSet<_> = right.keys().collect();

            let all_keys: HashSet<_> = left_keys.union(&right_keys).collect();
            let mut diffs = Vec::new();
            for key in all_keys {
                let inner_key = (*key).clone().data;
                let key_segment = Segment::try_from(key.data.clone()).unwrap();
                let path = ctx.path.push(key_segment);
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
                        let inner_key_segment = Segment::try_from(inner_key).unwrap();
                        diffs.append(&mut diff(ctx.for_key(inner_key_segment), left, right));
                    }
                }
            }
            diffs
        }
        (YamlDataOwned::Sequence(left_elements), YamlDataOwned::Sequence(right_elements)) => {
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
                // TODO: Optimize this O(n²) approach for large arrays - consider using LCS or similar algorithms
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
        _ => {
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

/// Take in a matrix of differences and produce a set of indices that minimize it
// TODO: Break down this complex function into smaller, more manageable pieces
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
    use saphyr::{LoadableYamlNode, Scalar};

    use crate::diff::ArrayOrdering;

    use super::{Context, Difference, Path, diff};

    #[test]
    fn simple_values_changes() {
        let left = saphyr::MarkedYamlOwned::load_from_str(indoc! {r#"
        foo:
          bar: 1
        "#})
        .unwrap();

        let right = saphyr::MarkedYamlOwned::load_from_str(indoc! {r#"
        foo:
          bar: 2
        "#})
        .unwrap();

        let differences = diff(Context::new(), &left[0], &right[0]);

        assert_eq!(
            differences,
            vec![Difference::Changed {
                left: saphyr::MarkedYamlOwned::from_bare_yaml(saphyr::Yaml::Value(
                    Scalar::Integer(1)
                )),
                right: saphyr::MarkedYamlOwned::from_bare_yaml(saphyr::Yaml::Value(
                    Scalar::Integer(2)
                )),
                path: Path::from_unchecked(vec!["foo".into(), "bar".into(),])
            }]
        )
    }

    #[test]
    fn added_or_changed_element_in_array() {
        let left = saphyr::MarkedYamlOwned::load_from_str(indoc! {r#"
        foo:
          - a
          - b
          - c
        "#})
        .unwrap();

        let right = saphyr::MarkedYamlOwned::load_from_str(indoc! {r#"
        foo:
          - x
          - b
          - c
          - d
        "#})
        .unwrap();

        let differences = diff(Context::new(), &left[0], &right[0]);

        assert_eq!(
            differences,
            vec![
                Difference::Changed {
                    left: saphyr::MarkedYamlOwned::from_bare_yaml(saphyr::Yaml::Value(
                        Scalar::String("a".into())
                    )),
                    right: saphyr::MarkedYamlOwned::from_bare_yaml(saphyr::Yaml::Value(
                        Scalar::String("x".into())
                    )),
                    path: Path::from_unchecked(vec!["foo".into(), 0.into(),])
                },
                Difference::Added {
                    path: Path::from_unchecked(vec!["foo".into(), 3.into()]),
                    value: saphyr::MarkedYamlOwned::from_bare_yaml(saphyr::Yaml::Value(
                        Scalar::String("d".into())
                    )),
                }
            ]
        )
    }

    #[test]
    fn removed_element_in_vector() {
        let left = saphyr::MarkedYamlOwned::load_from_str(indoc! {r#"
        foo:
          - a
          - b
          - c
        "#})
        .unwrap();

        let right = saphyr::MarkedYamlOwned::load_from_str(indoc! {r#"
        foo:
          - a
          - b
        "#})
        .unwrap();

        let differences = diff(Context::new(), &left[0], &right[0]);

        assert_eq!(
            differences,
            vec![Difference::Removed {
                path: Path::from_unchecked(vec!["foo".into(), 2.into()]),
                value: saphyr::MarkedYamlOwned::from_bare_yaml(saphyr::Yaml::Value(
                    Scalar::String("c".into())
                )),
            }]
        )
    }

    #[test]
    fn type_change() {
        let left = saphyr::MarkedYamlOwned::load_from_str(indoc! {r#"
        foo:
          bar: "12"
        "#})
        .unwrap();

        let right = saphyr::MarkedYamlOwned::load_from_str(indoc! {r#"
        foo:
          bar: false
        "#})
        .unwrap();

        let differences = diff(Context::new(), &left[0], &right[0]);

        assert_eq!(
            differences,
            vec![Difference::Changed {
                left: saphyr::MarkedYamlOwned::from_bare_yaml(saphyr::Yaml::Value(Scalar::String(
                    "12".into()
                ))),
                right: saphyr::MarkedYamlOwned::from_bare_yaml(saphyr::Yaml::Value(
                    Scalar::Boolean(false)
                )),
                path: Path::from_unchecked(vec!["foo".into(), "bar".into(),])
            },]
        )
    }

    #[test]
    fn netpol_example() {
        let left = saphyr::MarkedYamlOwned::load_from_str(indoc! {r#"
        egress:
          - ports:
            - port: 80
              protocol: TCP
            to:
            - ipBlock:
                cidr: 169.254.169.254/32
        "#})
        .unwrap();

        let right = saphyr::MarkedYamlOwned::load_from_str(indoc! {r#"
        egress:
          - to:
            - ipBlock:
                cidr: 169.254.169.254/32
            ports:
            - port: 80
        "#})
        .unwrap();
        let differences = diff(Context::new(), &left[0], &right[0]);

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
                value: saphyr::MarkedYamlOwned::from_bare_yaml(saphyr::Yaml::Value(
                    Scalar::String("TCP".into())
                )),
            }]
        )
    }

    #[test]
    fn detect_when_some_elements_have_been_moved_and_others_have_been_added() {
        let left = saphyr::MarkedYamlOwned::load_from_str(indoc! {r#"
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

        let right = saphyr::MarkedYamlOwned::load_from_str(indoc! {r#"
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

        let differences = diff(ctx, &left[0], &right[0]);
        expect![[r#"
            [
                Added {
                    path: Path(
                        [
                            Field(
                                "some_list",
                            ),
                            Index(
                                1,
                            ),
                        ],
                    ),
                    value: MarkedYamlOwned {
                        span: Span {
                            start: Marker {
                                index: 73,
                                line: 6,
                                col: 4,
                            },
                            end: Marker {
                                index: 130,
                                line: 10,
                                col: 2,
                            },
                        },
                        data: Mapping(
                            {
                                MarkedYamlOwned {
                                    span: Span {
                                        start: Marker {
                                            index: 73,
                                            line: 6,
                                            col: 4,
                                        },
                                        end: Marker {
                                            index: 77,
                                            line: 6,
                                            col: 8,
                                        },
                                    },
                                    data: Value(
                                        String(
                                            "name",
                                        ),
                                    ),
                                }: MarkedYamlOwned {
                                    span: Span {
                                        start: Marker {
                                            index: 79,
                                            line: 6,
                                            col: 10,
                                        },
                                        end: Marker {
                                            index: 85,
                                            line: 6,
                                            col: 16,
                                        },
                                    },
                                    data: Value(
                                        String(
                                            "lambda",
                                        ),
                                    ),
                                },
                                MarkedYamlOwned {
                                    span: Span {
                                        start: Marker {
                                            index: 90,
                                            line: 7,
                                            col: 4,
                                        },
                                        end: Marker {
                                            index: 95,
                                            line: 7,
                                            col: 9,
                                        },
                                    },
                                    data: Value(
                                        String(
                                            "value",
                                        ),
                                    ),
                                }: MarkedYamlOwned {
                                    span: Span {
                                        start: Marker {
                                            index: 103,
                                            line: 8,
                                            col: 6,
                                        },
                                        end: Marker {
                                            index: 130,
                                            line: 10,
                                            col: 2,
                                        },
                                    },
                                    data: Mapping(
                                        {
                                            MarkedYamlOwned {
                                                span: Span {
                                                    start: Marker {
                                                        index: 103,
                                                        line: 8,
                                                        col: 6,
                                                    },
                                                    end: Marker {
                                                        index: 109,
                                                        line: 8,
                                                        col: 12,
                                                    },
                                                },
                                                data: Value(
                                                    String(
                                                        "wheels",
                                                    ),
                                                ),
                                            }: MarkedYamlOwned {
                                                span: Span {
                                                    start: Marker {
                                                        index: 111,
                                                        line: 8,
                                                        col: 14,
                                                    },
                                                    end: Marker {
                                                        index: 112,
                                                        line: 8,
                                                        col: 15,
                                                    },
                                                },
                                                data: Value(
                                                    Integer(
                                                        9,
                                                    ),
                                                ),
                                            },
                                            MarkedYamlOwned {
                                                span: Span {
                                                    start: Marker {
                                                        index: 119,
                                                        line: 9,
                                                        col: 6,
                                                    },
                                                    end: Marker {
                                                        index: 124,
                                                        line: 9,
                                                        col: 11,
                                                    },
                                                },
                                                data: Value(
                                                    String(
                                                        "doors",
                                                    ),
                                                ),
                                            }: MarkedYamlOwned {
                                                span: Span {
                                                    start: Marker {
                                                        index: 126,
                                                        line: 9,
                                                        col: 13,
                                                    },
                                                    end: Marker {
                                                        index: 127,
                                                        line: 9,
                                                        col: 14,
                                                    },
                                                },
                                                data: Value(
                                                    Integer(
                                                        9,
                                                    ),
                                                ),
                                            },
                                        },
                                    ),
                                },
                            },
                        ),
                    },
                },
                Moved {
                    original_path: Path(
                        [
                            Field(
                                "some_list",
                            ),
                            Index(
                                1,
                            ),
                        ],
                    ),
                    new_path: Path(
                        [
                            Field(
                                "some_list",
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
                                "some_list",
                            ),
                            Index(
                                0,
                            ),
                            Field(
                                "value",
                            ),
                            Field(
                                "doors",
                            ),
                        ],
                    ),
                    left: MarkedYamlOwned {
                        span: Span {
                            start: Marker {
                                index: 67,
                                line: 5,
                                col: 13,
                            },
                            end: Marker {
                                index: 68,
                                line: 5,
                                col: 14,
                            },
                        },
                        data: Value(
                            Integer(
                                1,
                            ),
                        ),
                    },
                    right: MarkedYamlOwned {
                        span: Span {
                            start: Marker {
                                index: 244,
                                line: 17,
                                col: 13,
                            },
                            end: Marker {
                                index: 245,
                                line: 17,
                                col: 14,
                            },
                        },
                        data: Value(
                            Integer(
                                2,
                            ),
                        ),
                    },
                },
            ]
        "#]]
        .assert_debug_eq(&differences);
    }
}
