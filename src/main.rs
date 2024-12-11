use clap::Parser;
use std::{cmp::max, collections::HashSet};

mod multidoc;

/// Differnece between YAML documents
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    multidoc: bool,
    left: camino::Utf8PathBuf,
    right: camino::Utf8PathBuf,
}

fn main() {
    let args = Args::parse();

    let left = std::fs::File::open(args.left).unwrap();
    let left_doc: serde_yaml::Value = serde_yaml::from_reader(left).unwrap();

    let right = std::fs::File::open(args.right).unwrap();
    let right_doc: serde_yaml::Value = serde_yaml::from_reader(right).unwrap();

    let diffs = diff(Context::new(), &left_doc, &right_doc);

    dbg!(diffs);
}

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Context {
    path: Path,
}

impl Default for Context {
    fn default() -> Self {
        Self {
            path: Path(vec![".".into()]),
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
            // we start by comparing the in order
            let max_element_idx = max(left_elements.len(), right_elements.len());

            let mut diffs = Vec::new();
            for idx in 0..max_element_idx {
                match (left_elements.get(idx), right_elements.get(idx)) {
                    (None, None) => unreachable!("the index must be from either left or right!"),
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

    use crate::{diff, Context, Difference, Path};

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
                path: Path::from_unchecked(vec![".".into(), "foo".into(), "bar".into(),])
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
                    path: Path::from_unchecked(vec![".".into(), "foo".into(), 0.into(),])
                },
                Difference::Added {
                    path: Path::from_unchecked(vec![".".into(), "foo".into(), 3.into()]),
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
                path: Path::from_unchecked(vec![".".into(), "foo".into(), 2.into()]),
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
                path: Path::from_unchecked(vec![".".into(), "foo".into(), "bar".into(),])
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
                    ".".into(),
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
}
