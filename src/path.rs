use std::str::FromStr;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Segment {
    Field(String),
    Index(usize),
}

impl Segment {
    pub fn as_field(&self) -> Option<String> {
        match self {
            Segment::Field(f) => Some(f.to_string()),
            Segment::Index(_) => None,
        }
    }

    pub fn as_index(&self) -> Option<usize> {
        match self {
            Segment::Field(_) => None,
            Segment::Index(idx) => Some(*idx),
        }
    }

    pub fn as_yaml(&self) -> MarkedYamlOwned {
        match self {
            Segment::Field(f) => MarkedYamlOwned::value_from_str(f.as_str()),
            Segment::Index(i) => MarkedYamlOwned {
                span: Default::default(),
                data: saphyr::YamlDataOwned::Value(saphyr::ScalarOwned::Integer(*i as i64)),
            },
        }
    }
}

impl From<&str> for Segment {
    fn from(value: &str) -> Self {
        Segment::Field(value.to_string())
    }
}

impl TryFrom<saphyr::YamlDataOwned<MarkedYamlOwned>> for Segment {
    type Error = anyhow::Error;

    fn try_from(value: saphyr::YamlDataOwned<MarkedYamlOwned>) -> Result<Self, Self::Error> {
        if let Some(f) = value.as_str() {
            return Ok(Segment::Field(f.to_string()));
        }
        if let Some(n) = value.as_integer() {
            return Ok(Segment::Index(n as usize));
        }
        anyhow::bail!("Only YAML strings and numbers can be turned into Segments")
    }
}

impl From<usize> for Segment {
    fn from(val: usize) -> Self {
        Segment::Index(val)
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Path(Vec<Segment>);

impl Path {
    pub fn head(&self) -> Option<&Segment> {
        self.0.last()
    }

    pub fn parent(&self) -> Option<Self> {
        if self.0.is_empty() {
            return None;
        }

        let mut copy = self.0.clone();
        copy.pop();

        Some(Path(copy))
    }
    pub fn jq_like(&self) -> String {
        let mut buf = String::new();
        for s in &self.0 {
            match s {
                Segment::Field(s) => {
                    buf += &format!(".{s}");
                }
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

    #[cfg(test)]
    pub fn parse_str(val: &'static str) -> Self {
        let segments = val
            .split(".")
            .skip(1) // paths start with a `.` at their root
            .map(|raw_segment| {
                if raw_segment.contains("[") {
                    let num = raw_segment.trim_start_matches("[").trim_end_matches("]");
                    Segment::Index(num.parse().expect("the segment to have a valid number"))
                } else {
                    Segment::Field(raw_segment.to_string())
                }
            })
            .collect();

        Self(segments)
    }

    pub fn segments(&self) -> &[Segment] {
        &self.0
    }
}

#[derive(Debug, Eq, PartialEq, Clone)]
enum MatchElement {
    Root,
    Field(String),
    Index(usize),
    AnyArrayElement,
}

impl MatchElement {
    fn matches(&self, segment: &Segment) -> bool {
        match (self, segment) {
            (MatchElement::Field(a), Segment::Field(b)) => a == b,
            (MatchElement::Index(a), Segment::Index(b)) => a == b,
            (MatchElement::AnyArrayElement, Segment::Index(_)) => true,
            _ => false,
        }
    }
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct IgnorePath(Vec<MatchElement>);

impl IgnorePath {
    fn absolute(&self) -> bool {
        matches!(self.0[0], MatchElement::Root)
    }

    pub fn matches(&self, path: &Path) -> bool {
        if self.absolute() {
            for (idx, element) in self.0.iter().skip(1).enumerate() {
                let Some(segment) = path.0.get(idx) else {
                    return false;
                };
                if !element.matches(segment) {
                    return false;
                }
            }
        } else {
            // let's find a start of a match... maybe!
            let start_element = self.0.first().unwrap();
            let Some(match_start) = path
                .segments()
                .iter()
                .position(|s| start_element.matches(s))
            else {
                return false;
            };
            // now that we have a start, the remaining of `self` needs to match too!
            for (p, q) in path.segments().iter().skip(match_start).zip(self.0.iter()) {
                if !q.matches(p) {
                    return false;
                }
            }
        }
        true
    }
}

impl FromStr for IgnorePath {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Ok((_, value)) = ignore_path(s) {
            return Ok(value);
        }
        bail!("Failed to aprse PathMatch")
    }
}

use anyhow::bail;
use nom::branch::alt;
use nom::bytes::complete::take_while1;
use nom::character::complete::char;
use nom::combinator::{map, opt};
use nom::multi::many0;
use nom::sequence::{delimited, preceded};
use nom::{IResult, Parser};
use saphyr::MarkedYamlOwned;

fn ignore_path(input: &str) -> IResult<&str, IgnorePath> {
    let mut segments = Vec::new();
    let (rest, root) = opt(char('.')).parse(input)?;
    if root.is_some() {
        segments.push(MatchElement::Root);
    }
    // the `.` is not required here as we've already consumed it for the Root.
    let (rest, first) = alt((text_field, escaped_field)).parse(rest)?;
    segments.push(first);

    let dot_field = preceded(char('.'), text_field);
    let field = alt((dot_field, escaped_field));

    // remaining fields...
    let (rest, mut elements) = many0(field).parse(rest)?;
    segments.append(&mut elements);
    Ok((rest, IgnorePath(segments)))
}

fn text_field(input: &str) -> IResult<&str, MatchElement> {
    let (rest, p) = take_while1(|c: char| c.is_ascii_alphabetic())(input)?;
    Ok((rest, MatchElement::Field(p.to_string())))
}

fn escaped_field(input: &str) -> IResult<&str, MatchElement> {
    let dotted_field_name = map(
        delimited(
            char('"'),
            take_while1(|c: char| c.is_ascii_alphabetic() || c == '.' || c == '/'),
            char('"'),
        ),
        |v: &str| MatchElement::Field(v.to_string()),
    );

    let array_index = map(take_while1(|c: char| c.is_ascii_digit()), |v: &str| {
        MatchElement::Index(v.parse::<usize>().unwrap())
    });
    let any_array_index = map(char('*'), |_| MatchElement::AnyArrayElement);
    let (rest, p) = delimited(
        char('['),
        alt((dotted_field_name, array_index, any_array_index)),
        char(']'),
    )
    .parse(input)?;

    Ok((rest, p))
}

#[cfg(test)]
mod path_match_parsing {
    use pretty_assertions::assert_eq;

    use crate::path::MatchElement;

    use super::IgnorePath;
    use std::str::FromStr;

    #[test]
    pub fn can_be_read_from_string() {
        struct Case {
            input: &'static str,
            expected: IgnorePath,
        }
        let cases = vec![
            Case {
                input: r#".spec"#,
                expected: IgnorePath(vec![
                    MatchElement::Root,
                    MatchElement::Field("spec".to_string()),
                ]),
            },
            Case {
                input: r#"spec.annotations"#,
                expected: IgnorePath(vec![
                    MatchElement::Field("spec".to_string()),
                    MatchElement::Field("annotations".to_string()),
                ]),
            },
            Case {
                input: r#"spec.annotations["app.kubernetes.io/name"]"#,
                expected: IgnorePath(vec![
                    MatchElement::Field("spec".to_string()),
                    MatchElement::Field("annotations".to_string()),
                    MatchElement::Field("app.kubernetes.io/name".to_string()),
                ]),
            },
            Case {
                input: r#"spec.env[1]"#,
                expected: IgnorePath(vec![
                    MatchElement::Field("spec".to_string()),
                    MatchElement::Field("env".to_string()),
                    MatchElement::Index(1),
                ]),
            },
            Case {
                input: r#"spec.env[*].name"#,
                expected: IgnorePath(vec![
                    MatchElement::Field("spec".to_string()),
                    MatchElement::Field("env".to_string()),
                    MatchElement::AnyArrayElement,
                    MatchElement::Field("name".to_string()),
                ]),
            },
        ];

        for case in &cases {
            let matcher = IgnorePath::from_str(case.input).unwrap();
            assert_eq!(matcher, case.expected,)
        }
    }
}

#[cfg(test)]
mod path_ignoring {
    use std::str::FromStr;

    use crate::path::IgnorePath;

    use super::Path;

    #[test]
    pub fn matching_paths_with_ignore_paths_structs() {
        struct Case {
            path_match: &'static str,
            path: Path,
            matches: bool,
        }

        let cases = [
            Case {
                path_match: ".spec.annotations",
                path: Path::default()
                    .push("spec")
                    .push("annotations")
                    .push("foo.bar.com"),
                matches: true,
            },
            Case {
                path_match: "annotations",
                path: Path::default()
                    .push("spec")
                    .push("annotations")
                    .push("foo.bar.com"),
                matches: true,
            },
            Case {
                path_match: "spec.env[3].name",
                path: Path::default()
                    .push("spec")
                    .push("env")
                    .push(3)
                    .push("name"),
                matches: true,
            },
            Case {
                path_match: "spec.env[*].name",
                path: Path::default()
                    .push("spec")
                    .push("env")
                    .push(3)
                    .push("name"),
                matches: true,
            },
            Case {
                path_match: r#"annotations["app.kubernetes.io/name"]"#,
                path: Path::default()
                    .push("spec")
                    .push("template")
                    .push("metadata")
                    .push("annotations")
                    .push("app.kubernetes.io/name"),
                matches: true,
            },
        ];

        for case in cases.iter().skip(4) {
            let path_match = IgnorePath::from_str(case.path_match).unwrap();

            assert_eq!(case.matches, path_match.matches(&case.path));
        }
    }
}
