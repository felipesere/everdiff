use anyhow::bail;
use json_patch::PatchOperation;
use jsonptr::resolve::ResolveError;
use saphyr::{LoadableYamlNode, MarkedYamlOwned, Yaml, YamlDataOwned};
use serde::Deserialize;

use crate::YamlSource;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Value to patch not found")]
    ValueNotFoundAtPath,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrePatch {
    #[allow(dead_code)]
    name: Option<String>,
    document_like: Option<serde_json::Value>,
    patches: json_patch::Patch,
}

impl PrePatch {
    pub fn apply_to(&self, documents: &mut Vec<YamlSource>) -> Result<(), Error> {
        for doc in documents {
            if let Some(doc_matcher) = &self.document_like
                && !document_matches(doc_matcher, &doc.yaml)
            {
                continue;
            }
            apply_patch(&self.patches, &mut doc.yaml)?;
        }

        Ok(())
    }
}

// Shamelessly stolen from jsontr::Pointer.
// It comes from a `Resolve` trait which is implemented for `serde_json::Value` and TOML
// but sadly not for `serde_json::Value`.
/// Get mutable access to the Value that `ptr` points at within `value`.
fn resolve_mut<'a>(
    mut value: &'a mut MarkedYamlOwned,
    mut ptr: &jsonptr::Pointer,
) -> Result<&'a mut MarkedYamlOwned, anyhow::Error> {
    let mut offset = 0;
    let mut position = 0;
    while let Some((token, rem)) = ptr.split_front() {
        let tok_len = token.encoded().len();
        ptr = rem;

        value = if value.is_sequence() {
            let items = value.data.as_sequence_mut().unwrap();
            let idx = token
                .to_index()
                .map_err(|source| ResolveError::FailedToParseIndex {
                    offset,
                    source,
                    position,
                })?
                .for_len(items.len())
                .map_err(|source| ResolveError::OutOfBounds {
                    offset,
                    source,
                    position,
                })?;
            &mut items[idx]
        } else if value.is_mapping() {
            let items = value.data.as_mapping_mut().unwrap();
            let token = token.decoded().to_string();
            let key = MarkedYamlOwned::from_bare_yaml(Yaml::value_from_str(&token));
            &mut items[&key]
        } else {
            // return Err(ResolveError::Unreachable { offset }.).;
            bail!("This totally failed!");
        };
        offset += 1 + tok_len;
        position += 1;
    }
    Ok(value)
}

fn apply_patch(patches: &json_patch::Patch, doc: &mut MarkedYamlOwned) -> Result<(), Error> {
    for p in patches.iter() {
        match p {
            PatchOperation::Replace(r) => {
                if let Ok(v) = resolve_mut(doc, &r.path) {
                    let the_yaml = serde_json::to_string(&r.value)
                        .expect("should turn patch value into yaml string");
                    let replacement = MarkedYamlOwned::load_from_str(the_yaml.as_str())
                        .expect("valid yaml?")
                        .remove(0);
                    *v = replacement;
                } else {
                    return Err(Error::ValueNotFoundAtPath);
                }
            }
            PatchOperation::Add(a) => {
                if let Some((path, field)) = a.path.split_back() {
                    if let Ok(v) = resolve_mut(doc, path) {
                        if let Some(m) = v.data.as_mapping_mut() {
                            let the_yaml = serde_json::to_string(&a.value)
                                .expect("should turn patch value into yaml string");
                            let replacement = MarkedYamlOwned::load_from_str(the_yaml.as_str())
                                .expect("valid yaml?")
                                .remove(0);
                            let key = MarkedYamlOwned::value_from_str(field.to_string().as_ref());
                            m.insert(key, replacement);
                        };
                    } else {
                        return Err(Error::ValueNotFoundAtPath);
                    }
                }
            }
            _ => unimplemented!("We only currently support add & replace"),
        }
    }
    Ok(())
}

fn document_matches(document_like: &serde_json::Value, actual_doc: &MarkedYamlOwned) -> bool {
    match (document_like, &actual_doc.data) {
        (serde_json::Value::Null, YamlDataOwned::Value(saphyr::ScalarOwned::Null)) => true,
        (serde_json::Value::Bool(a), YamlDataOwned::Value(saphyr::ScalarOwned::Boolean(b))) => {
            a == b
        }
        (serde_json::Value::Number(n), YamlDataOwned::Value(saphyr::ScalarOwned::Integer(b)))
            if n.is_i64() =>
        {
            n.as_i64().unwrap() == *b
        }
        (
            serde_json::Value::Number(n),
            YamlDataOwned::Value(saphyr::ScalarOwned::FloatingPoint(b)),
        ) if n.is_f64() => {
            let a = n.as_f64().unwrap();
            let b = b.into_inner();
            a == b
        }
        (serde_json::Value::String(a), YamlDataOwned::Value(saphyr::ScalarOwned::String(b))) => {
            a == b
        }
        (serde_json::Value::Array(required), YamlDataOwned::Sequence(available)) => {
            for (r, a) in required.iter().zip(available.iter()) {
                if !document_matches(r, a) {
                    return false;
                }
            }
            true
        }
        (serde_json::Value::Object(required), YamlDataOwned::Mapping(available)) => {
            for (key, value) in required {
                let key = MarkedYamlOwned::value_from_str(key.as_str());
                let Some(other_value) = available.get(&key) else {
                    return false;
                };
                if !document_matches(value, other_value) {
                    return false;
                }
            }

            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use expect_test::expect;
    use indoc::indoc;
    use saphyr::YamlEmitter;

    use crate::{YamlSource, node::to_value, read_doc};

    use super::PrePatch;

    #[test]
    fn can_read_a_patch_statement() {
        let raw = indoc! {r#"
            name: rename network policy to match chart convention
            # documentIndex: 4
            documentLike:
              kind: NetworkPolicy
              metadata:
                name: flux-engine-steam
            patches:
              - op: replace
                path: "/metadata/name"
                value: "flux"
        "#};

        let _pp: PrePatch = serde_saphyr::from_str(raw).unwrap();
    }

    #[test]
    fn replaces_the_name_of_the_networkpolicy_to_match() {
        let raw_docs = indoc! {r#"
        ---
        kind: NetworkPolicy
        metadata:
          name:  flux-engine-steam
        ---
        kind: NetworkPolicy
        metadata:
          name:  the-other-one
        "#};

        let mut documents = docs(raw_docs);
        let pp: PrePatch = serde_saphyr::from_str(indoc! {r#"
            name: rename network policy to match chart convention
            # documentIndex: 4
            documentLike:
              kind: NetworkPolicy
              metadata:
                name: flux-engine-steam
            patches:
              - op: replace
                path: "/metadata/name"
                value: "flux"
        "#})
        .unwrap();

        let _ = pp.apply_to(&mut documents);

        let outcome = serialize(&documents);
        expect![[r#"
            ---
            kind: NetworkPolicy
            metadata:
              name: flux
            ---
            kind: NetworkPolicy
            metadata:
              name: the-other-one
        "#]]
        .assert_eq(&outcome);
    }

    #[test]
    fn adds_the_namespace_to_all_documents() {
        let raw_docs = indoc! {r#"
        ---
        kind: NetworkPolicy
        metadata:
          name: flux-engine-steam
        ---
        kind: Deployment
        metadata:
          name: the-other-one
        "#};

        let mut documents = docs(raw_docs);
        let pp: PrePatch = serde_saphyr::from_str(indoc! {r#"
            name: Add the namespace everywhere
            patches:
              - op: add
                path: /metadata/namespace
                value: core
        "#})
        .unwrap();

        pp.apply_to(&mut documents).unwrap();

        let outcome = serialize(&documents);
        expect![[r#"
            ---
            kind: NetworkPolicy
            metadata:
              name: flux-engine-steam
              namespace: core
            ---
            kind: Deployment
            metadata:
              name: the-other-one
              namespace: core
        "#]]
        .assert_eq(&outcome);
    }

    pub fn docs(raw: &str) -> Vec<YamlSource> {
        read_doc(raw, camino::Utf8PathBuf::new()).unwrap()
    }

    pub fn serialize(docs: &[YamlSource]) -> String {
        let mut out_str = String::new();
        for doc in docs {
            {
                let mut emitter = YamlEmitter::new(&mut out_str);
                emitter.dump(&to_value(&doc.yaml)).unwrap();
            }
            out_str.push('\n');
        }
        out_str
    }
}
