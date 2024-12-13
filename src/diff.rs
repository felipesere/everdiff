use std::collections::HashSet;

#[derive(Debug, Eq, PartialEq)]
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
}

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Path(Vec<Segment>);

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrayOrdering {
    Fixed,
    Dynamic,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Context {
    path: Path,
    array_ordering: ArrayOrdering,
}

impl Default for Context {
    fn default() -> Self {
        Self {
            path: Path(vec![]),
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
                let mut total_difference = Vec::new();
                for (idx, left_value) in left_elements.iter().enumerate() {
                    let mut best_fit: Option<(usize, usize, Vec<Difference>)> = None;
                    for (rdx, right_value) in right_elements.iter().enumerate() {
                        let difference = diff(ctx.for_key(idx), left_value, right_value);
                        match best_fit {
                            None => {
                                best_fit = Some((idx, rdx, difference));
                            }
                            Some((_, _, ref before)) if before.len() > difference.len() => {
                                best_fit = Some((idx, rdx, difference));
                            }
                            _ => {}
                        }
                    }
                    if let Some((_, _, mut difference)) = best_fit {
                        total_difference.append(&mut difference);
                    }
                }

                // dynamic ordering... so find best matches!
                total_difference
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

#[cfg(test)]
mod tests {
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

    /*
     *       A   B   C   D
     *    1  o   x   x   x
     *    2  x   x   o   x
     *    3  x   x   x   o
     *
     *
     */

    #[test]
    fn reordered_array_should_still_be_equal() {
        let left = serde_yaml::from_str(indoc! {r#"
        some_list:
          - name: alpha
            value:
              wheels: 5
              doors: 3
          - name: bravo
            value:
              wheels: 5
              doors: 3
          - name: charlie
            value:
              wheels: 5
              doors: 3
        "#})
        .unwrap();

        let right = serde_yaml::from_str(indoc! {r#"
        some_list:
          - value:
              wheels: 5
              doors: 3
            name: bravo
          - name: charlie
            value:
              doors: 3
              wheels: 5
          - name: alpha
            value:
              wheels: 5
              doors: 3
        "#})
        .unwrap();

        let mut ctx = Context::new();
        ctx.array_ordering = ArrayOrdering::Dynamic;

        let differences = diff(ctx, &left, &right);
        assert_eq!(differences, Vec::new());
    }
}
