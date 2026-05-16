use std::cmp::Ordering;
use std::{collections::BTreeMap, fmt::Display};

use everdiff_diff::{ArrayOrdering, Context as DiffContext, Difference as Diff, diff as diff_yaml};

use crate::source::YamlSource;

pub mod source;

/// Fn that identifies a document by inspecting keys
pub type IdentifierFn = Box<dyn Fn(usize, &YamlSource) -> Option<Fields>>;

/// The underlying file path and the index _in_ that file.
/// In YAML a file can contain multiple documents separated by
/// `---` and `...`.
pub type DocumentRef = (camino::Utf8PathBuf, usize);

/// Two matching documents, they have the same output for `Fields`
#[derive(Debug)]
pub struct MatchingDocs {
    /// Fields used that matched
    fields: Fields,

    /// The left document from the match
    left: DocumentRef,
    ///
    /// The right document from the match
    right: DocumentRef,
}

#[derive(Debug, Eq, PartialEq)]
pub struct MissingDoc {
    pub doc: DocumentRef,
    pub fields: Fields,
}

#[derive(Debug, Eq, PartialEq)]
pub struct AdditionalDoc {
    pub doc: DocumentRef,
    pub fields: Fields,
}

pub struct Context {
    identifier: IdentifierFn,
}

impl std::fmt::Debug for Context {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Context")
            .field("doc_identifier", &"a fn")
            .finish()
    }
}

impl Context {
    pub fn new_with_doc_identifier(identifier: IdentifierFn) -> Self {
        Context { identifier }
    }
}

// TODO: Consider if we can use [iddqd](https://docs.rs/iddqd/latest/iddqd/) could spare us some clones
fn matching_docs(
    lefts: &[YamlSource],
    rights: &[YamlSource],
    extract: &IdentifierFn,
) -> (Vec<MatchingDocs>, Vec<MissingDoc>, Vec<AdditionalDoc>) {
    let mut seen_left_docs: BTreeMap<Fields, DocumentRef> = BTreeMap::new();
    let mut seen_right_docs: BTreeMap<Fields, DocumentRef> = BTreeMap::new();
    let mut matches = Vec::new();
    let mut missing_docs = Vec::new();
    let mut added_docs: Vec<AdditionalDoc> = Vec::new();

    let mut last_idx_used_on_right = 0_usize;
    'comparing_left_docs: for (index, doc) in lefts.iter().enumerate() {
        if let Some(fields) = extract(index, doc) {
            seen_left_docs.insert(fields.clone(), (doc.file.clone(), index));
            if let Some(right_ref) = seen_right_docs.get(&fields) {
                matches.push(MatchingDocs {
                    fields,
                    left: (doc.file.clone(), index),
                    right: right_ref.clone(),
                });
                continue 'comparing_left_docs;
            }

            for (right, right_doc) in rights.iter().enumerate().skip(last_idx_used_on_right) {
                if let Some(right_fields) = extract(right, right_doc) {
                    seen_right_docs.insert(right_fields.clone(), (right_doc.file.clone(), right));
                    if fields == right_fields {
                        matches.push(MatchingDocs {
                            fields,
                            left: (doc.file.clone(), index),
                            right: (right_doc.file.clone(), right),
                        });
                        last_idx_used_on_right = right;
                        continue 'comparing_left_docs;
                    }
                }
            }
            // ...we've gone through all the docs on the "right" without finding a match, it must
            // be missing
            missing_docs.push(MissingDoc {
                doc: (doc.file.clone(), index),
                fields,
            })
        }
    }
    // Ensure every right doc is in seen_right_docs, including those that appear after the
    // last matched index and were therefore never visited by the inner scan loop.
    for (right, right_doc) in rights.iter().enumerate().skip(last_idx_used_on_right) {
        if let Some(right_fields) = extract(right, right_doc) {
            seen_right_docs
                .entry(right_fields)
                .or_insert_with(|| (right_doc.file.clone(), right));
        }
    }

    // let's go over all docs we've seen on the right and check which ones don't exist on the left
    for (fields, right_ref) in seen_right_docs {
        if seen_left_docs.contains_key(&fields) {
            continue;
        }
        added_docs.push(AdditionalDoc {
            doc: right_ref,
            fields,
        })
    }

    (matches, missing_docs, added_docs)
}

/// Newtype used to identify a document.
/// Two Documents that produce the same `Fields` will be diffed
/// against each other.
/// A common use case is to for example grab
/// * apiVersion
/// * kind
/// * metadata.name
///
/// from a Kubernetes resource to diff
// TODO: Add a proper consstructor and some APIs
#[derive(Default, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Fields {
    idx: Option<usize>,
    inner: BTreeMap<String, Option<String>>,
}

impl Fields {
    pub fn new() -> Self {
        Fields::default()
    }

    pub fn with_idx(mut self, idx: usize) -> Self {
        self.idx = Some(idx);
        self
    }

    pub fn with(mut self, key: impl Into<String>, value: impl Into<Option<String>>) -> Self {
        self.inner.insert(key.into(), value.into());
        self
    }

    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<Option<String>>) {
        self.inner.insert(key.into(), value.into());
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

impl Display for Fields {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (k, v) in &self.inner {
            f.write_fmt(format_args!(
                "{k} -> {value}\n",
                value = v.as_deref().unwrap_or("∅")
            ))?;
        }
        Ok(())
    }
}

impl AsRef<BTreeMap<String, Option<String>>> for Fields {
    fn as_ref(&self) -> &BTreeMap<String, Option<String>> {
        &self.inner
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum DocDifference {
    Addition(AdditionalDoc),
    Missing(MissingDoc),
    Changed {
        left: DocumentRef,
        right: DocumentRef,
        fields: Fields,
        differences: Vec<Diff>,
    },
}

impl PartialOrd for DocDifference {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DocDifference {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (
                DocDifference::Addition(AdditionalDoc { fields, .. }),
                DocDifference::Addition(AdditionalDoc { fields: other, .. }),
            ) => fields.cmp(other),
            (
                DocDifference::Missing(MissingDoc { fields, .. }),
                DocDifference::Missing(MissingDoc {
                    fields: other_fields,
                    ..
                }),
            ) => fields.cmp(other_fields),
            (
                DocDifference::Changed { fields, .. },
                DocDifference::Changed {
                    fields: other_fields,
                    ..
                },
            ) => fields.cmp(other_fields),
            (DocDifference::Addition(_), _) => Ordering::Less,
            (DocDifference::Changed { .. }, _) => Ordering::Greater,
            (DocDifference::Missing(_), DocDifference::Addition(_)) => Ordering::Greater,
            (DocDifference::Missing(_), DocDifference::Changed { .. }) => Ordering::Less,
        }
    }
}

pub fn diff(ctx: &Context, lefts: &[YamlSource], rights: &[YamlSource]) -> Vec<DocDifference> {
    let (matches, missing, added) = matching_docs(lefts, rights, &ctx.identifier);

    let mut differences = Vec::new();
    for MatchingDocs {
        fields,
        left,
        right,
    } in matches
    {
        let left_doc = &lefts[left.1].yaml;
        let right_doc = &rights[right.1].yaml;
        let mut diff_context = DiffContext::new();
        diff_context.array_ordering = ArrayOrdering::Dynamic;

        let diffs = diff_yaml(diff_context, left_doc, right_doc);
        if !diffs.is_empty() {
            differences.push(DocDifference::Changed {
                fields,
                left,
                right,
                differences: diffs,
            })
        }
    }
    for m in missing {
        differences.push(DocDifference::Missing(m));
    }
    for a in added {
        differences.push(DocDifference::Addition(a));
    }
    differences
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use expect_test::expect;
    use pretty_assertions::assert_eq;

    use crate::{
        Context, DocDifference, Fields, diff,
        source::{YamlSource, read_doc},
    };
    use indoc::indoc;

    pub fn docs(raw: &str) -> Vec<YamlSource> {
        read_doc(
            raw,
            &camino::Utf8PathBuf::from_str("/foo/bar/baz.yaml").unwrap(),
        )
        .unwrap()
    }

    fn kubernetes_names() -> super::IdentifierFn {
        use saphyr::{MarkedYamlOwned, SafelyIndex};

        fn string_of(node: Option<&MarkedYamlOwned>) -> Option<String> {
            node?.data.as_str().map(String::from)
        }

        Box::new(|_idx, source| {
            let doc = &source.yaml;
            let name = string_of(doc.get("metadata")?.get("name"));
            let namespace = string_of(doc.get("metadata")?.get("namespace"));
            let fields = Fields::default()
                .with("metadata.name", name)
                .with("metadata.namespace", namespace);
            Some(fields)
        })
    }

    #[test]
    #[ignore = "compares debug structures that I am refactoring"]
    fn two_documents_changed_out_of_order() {
        let left = docs(indoc! {r#"
        ---
        metadata:
          name: bravo
        spec:
          color: yellow
        ...
        ---
        metadata:
          name: alpha
          namespace: ns
        spec:
          thing: 12
        ...
        ---
        metadata:
          name: charlie
        spec:
          wheels: 6
        ...
        "#});

        let right = docs(indoc! {r#"
        ---
        metadata:
          name: alpha
          namespace: ns
        spec:
          thing: 24
        ...
        ---
        metadata:
          name: bravo
        spec:
          color: blue
        ...
        ---
        metadata:
          name: delta
        spec:
          size: xl
        ...
        "#});

        let ctx = Context::new_with_doc_identifier(kubernetes_names());
        let differences = diff(&ctx, &left, &right);

        expect![[r#"
            [
                Changed {
                    key: DocKey {
                        src_file: "/foo/bar/baz.yaml",
                        fields: {
                            "metadata.name": Some(
                                "bravo",
                            ),
                            "metadata.namespace": None,
                        },
                    },
                    left_doc_idx: 0,
                    right_doc_idx: 1,
                    differences: [
                        Changed {
                            path: Some(
                                NonEmptyPath(
                                    Path(
                                        [
                                            Field(
                                                "spec",
                                            ),
                                            Field(
                                                "color",
                                            ),
                                        ],
                                    ),
                                ),
                            ),
                            left: MarkedYamlOwned {
                                span: Span {
                                    start: Marker {
                                        index: 43,
                                        line: 5,
                                        col: 9,
                                    },
                                    end: Marker {
                                        index: 49,
                                        line: 5,
                                        col: 15,
                                    },
                                },
                                data: Value(
                                    String(
                                        "yellow",
                                    ),
                                ),
                            },
                            right: MarkedYamlOwned {
                                span: Span {
                                    start: Marker {
                                        index: 109,
                                        line: 12,
                                        col: 9,
                                    },
                                    end: Marker {
                                        index: 113,
                                        line: 12,
                                        col: 13,
                                    },
                                },
                                data: Value(
                                    String(
                                        "blue",
                                    ),
                                ),
                            },
                        },
                    ],
                },
                Changed {
                    key: DocKey {
                        src_file: "/foo/bar/baz.yaml",
                        fields: {
                            "metadata.name": Some(
                                "alpha",
                            ),
                            "metadata.namespace": Some(
                                "ns",
                            ),
                        },
                    },
                    left_doc_idx: 1,
                    right_doc_idx: 0,
                    differences: [
                        Changed {
                            path: Some(
                                NonEmptyPath(
                                    Path(
                                        [
                                            Field(
                                                "spec",
                                            ),
                                            Field(
                                                "thing",
                                            ),
                                        ],
                                    ),
                                ),
                            ),
                            left: MarkedYamlOwned {
                                span: Span {
                                    start: Marker {
                                        index: 113,
                                        line: 12,
                                        col: 9,
                                    },
                                    end: Marker {
                                        index: 115,
                                        line: 12,
                                        col: 11,
                                    },
                                },
                                data: Value(
                                    Integer(
                                        12,
                                    ),
                                ),
                            },
                            right: MarkedYamlOwned {
                                span: Span {
                                    start: Marker {
                                        index: 59,
                                        line: 6,
                                        col: 9,
                                    },
                                    end: Marker {
                                        index: 61,
                                        line: 6,
                                        col: 11,
                                    },
                                },
                                data: Value(
                                    Integer(
                                        24,
                                    ),
                                ),
                            },
                        },
                    ],
                },
                Missing(
                    MissingDoc {
                        key: DocKey {
                            src_file: "/foo/bar/baz.yaml",
                            fields: {
                                "metadata.name": Some(
                                    "charlie",
                                ),
                                "metadata.namespace": None,
                            },
                        },
                        left: 2,
                    },
                ),
                Addition(
                    AdditionalDoc {
                        key: DocKey {
                            src_file: "/foo/bar/baz.yaml",
                            fields: {
                                "metadata.name": Some(
                                    "delta",
                                ),
                                "metadata.namespace": None,
                            },
                        },
                        right: 2,
                    },
                ),
            ]
        "#]]
        .assert_debug_eq(&differences);
    }

    // Bug 1: when right-side docs are out of order relative to the left, a left doc that
    // has a matching right doc is incorrectly reported as missing.
    //
    // Root cause: line 88 stored each right doc in `seen_right_docs` under the *left* doc's
    // fields instead of the right doc's own fields, so the early-exit lookup on line 77
    // (`seen_right_docs.get(&fields)`) never fires for later left docs.
    #[test]
    fn out_of_order_right_docs_are_not_falsely_missing() {
        // Left:  [alpha, bravo]
        // Right: [bravo, alpha]  — same docs, reversed order
        let left = docs(indoc! {r#"
        ---
        metadata:
          name: alpha
        spec:
          value: 1
        ...
        ---
        metadata:
          name: bravo
        spec:
          value: 2
        ...
        "#});

        let right = docs(indoc! {r#"
        ---
        metadata:
          name: bravo
        spec:
          value: 2
        ...
        ---
        metadata:
          name: alpha
        spec:
          value: 1
        ...
        "#});

        let ctx = Context::new_with_doc_identifier(kubernetes_names());
        let differences = diff(&ctx, &left, &right);

        // Both docs are identical (just reordered), so there should be no differences at all.
        assert_eq!(
            differences,
            vec![],
            "expected no differences for identical docs in different order, got: {differences:#?}"
        );
    }

    // Bug 2: right-only docs (additions) that appear *after* the position of the last matched
    // right doc are never visited in the inner scan loop, so they are silently dropped and
    // never reported as additions.
    //
    // Root cause: the inner loop skips from `last_idx_used_on_right` and only advances that
    // cursor when a match is found.  Right docs beyond the final match are never recorded in
    // `seen_right_docs`, and the final "additions" pass only inspects `seen_right_docs`.
    #[test]
    fn addition_at_end_of_right_is_detected() {
        // Left:  [alpha, bravo]
        // Right: [alpha, bravo, delta]  — delta is a new doc only on the right
        let left = docs(indoc! {r#"
        ---
        metadata:
          name: alpha
        spec:
          value: 1
        ...
        ---
        metadata:
          name: bravo
        spec:
          value: 2
        ...
        "#});

        let right = docs(indoc! {r#"
        ---
        metadata:
          name: alpha
        spec:
          value: 1
        ...
        ---
        metadata:
          name: bravo
        spec:
          value: 2
        ...
        ---
        metadata:
          name: delta
        spec:
          value: 99
        ...
        "#});

        let ctx = Context::new_with_doc_identifier(kubernetes_names());
        let differences = diff(&ctx, &left, &right);

        assert_eq!(
            differences.len(),
            1,
            "expected exactly one addition, got: {differences:#?}"
        );
        assert!(
            matches!(&differences[0], DocDifference::Addition(a) if a.fields.inner["metadata.name"] == Some("delta".to_string())),
            "expected an Addition for 'delta', got: {:#?}",
            differences[0]
        );
    }

    #[test]
    fn display_fields() {
        let fields = Fields::new()
            .with("api_version", "bar".to_string())
            .with("metadata.name", "foo".to_string());
        assert_eq!(
            fields.to_string(),
            indoc! {r#"
              api_version -> bar
              metadata.name -> foo
            "#}
        );
    }
}
