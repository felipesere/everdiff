use std::str::FromStr;

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

#[derive(Clone, Debug, Default, Eq, PartialEq)]
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

#[derive(Debug, Eq, PartialEq, Clone)]
enum MatchElement {
    Root,
    Field(String),
    Index(usize),
    AnyArrayElement,
}

#[derive(Debug, Eq, PartialEq, Clone)]
pub struct PathMatch(Vec<MatchElement>);

impl FromStr for PathMatch {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Ok((_, value)) = path_match(s) {
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
use nom::IResult;

fn path_match(input: &str) -> IResult<&str, PathMatch> {
    let mut segments = Vec::new();
    let (rest, root) = opt(char('.'))(input)?;
    if root.is_some() {
        segments.push(MatchElement::Root);
    }
    // the `.` is not required here as we've already consumed it for the Root.
    let (rest, first) = alt((parse_field, escaped_field))(rest)?;
    segments.push(first);

    let normal_field = preceded(char('.'), parse_field);
    let field = alt((normal_field, escaped_field));

    // remaining fields...
    let (rest, mut elements) = many0(field)(rest)?;
    segments.append(&mut elements);
    Ok((rest, PathMatch(segments)))
}

fn parse_field(input: &str) -> IResult<&str, MatchElement> {
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
    )(input)?;

    Ok((rest, p))
}

#[cfg(test)]
mod path_match_parsing {
    use pretty_assertions::assert_eq;

    use crate::path::MatchElement;

    use super::PathMatch;
    use std::str::FromStr;

    #[test]
    pub fn can_be_read_from_string() {
        struct Case {
            input: &'static str,
            expected: PathMatch,
        }
        let cases = vec![
            Case {
                input: r#".spec"#,
                expected: PathMatch(vec![
                    MatchElement::Root,
                    MatchElement::Field("spec".to_string()),
                ]),
            },
            Case {
                input: r#"spec.annotations"#,
                expected: PathMatch(vec![
                    MatchElement::Field("spec".to_string()),
                    MatchElement::Field("annotations".to_string()),
                ]),
            },
            Case {
                input: r#"spec.annotations["app.kubernetes.io/name"]"#,
                expected: PathMatch(vec![
                    MatchElement::Field("spec".to_string()),
                    MatchElement::Field("annotations".to_string()),
                    MatchElement::Field("app.kubernetes.io/name".to_string()),
                ]),
            },
            Case {
                input: r#"spec.env[1]"#,
                expected: PathMatch(vec![
                    MatchElement::Field("spec".to_string()),
                    MatchElement::Field("env".to_string()),
                    MatchElement::Index(1),
                ]),
            },
            Case {
                input: r#"spec.env[*].name"#,
                expected: PathMatch(vec![
                    MatchElement::Field("spec".to_string()),
                    MatchElement::Field("env".to_string()),
                    MatchElement::AnyArrayElement,
                    MatchElement::Field("name".to_string()),
                ]),
            },
        ];

        for case in &cases {
            let matcher = PathMatch::from_str(case.input).unwrap();
            assert_eq!(matcher, case.expected,)
        }
    }
}
