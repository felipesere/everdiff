use anyhow::bail;
use json_patch::PatchOperation;
use jsonptr::resolve::ResolveError;
use saphyr::{LoadableYamlNode, MarkedYaml, Yaml, YamlData};
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
    document_like: Option<serde_yaml::Value>,
    patches: json_patch::Patch,
}

impl PrePatch {
    pub fn apply_to(&self, documents: &mut Vec<YamlSource>) -> Result<(), Error> {
        for doc in documents {
            if let Some(doc_matcher) = &self.document_like {
                if !document_matches(doc_matcher, &doc.yaml) {
                    continue;
                }
            }
            apply_patch(&self.patches, &mut doc.yaml)?;
        }

        Ok(())
    }
}

// Shamelessly stolen from jsontr::Pointer.
// It comes from a `Resolve` trait which is implemented for `serde_json::Value` and TOML
// but sadly not for `serde_yaml::Value`.
// Get mutable access to the Value that `ptr` points at within `value`.
//
//fn resolve_mut<'a>(
//    mut value: &'a mut serde_yaml::Value,
//    mut ptr: &jsonptr::Pointer,
//) -> Result<&'a mut serde_yaml::Value, anyhow::Error> {
//    let mut offset = 0;
//    while let Some((token, rem)) = ptr.split_front() {
//        let tok_len = token.encoded().len();
//        ptr = rem;
//        value = match value {
//            Value::Sequence(v) => {
//                let idx = token
//                    .to_index()
//                    .map_err(|source| ResolveError::FailedToParseIndex { offset, source })?
//                    .for_len(v.len())
//                    .map_err(|source| ResolveError::OutOfBounds { offset, source })?;
//                Ok(v.get_mut(idx).unwrap())
//            }
//
//            Value::Mapping(v) => v
//                .get_mut(token.decoded().as_ref())
//                .ok_or(ResolveError::NotFound { offset }),
//            // found a leaf node but the pointer hasn't been exhausted
//            _ => Err(ResolveError::Unreachable { offset }),
//        }?;
//        offset += 1 + tok_len;
//    }
//    Ok(value)
//}

// Shamelessly stolen from jsontr::Pointer.
// It comes from a `Resolve` trait which is implemented for `serde_json::Value` and TOML
// but sadly not for `serde_yaml::Value`.
/// Get mutable access to the Value that `ptr` points at within `value`.
fn resolve_mut2<'a>(
    mut value: &'a mut MarkedYaml,
    mut ptr: &jsonptr::Pointer,
) -> Result<&'a mut MarkedYaml, anyhow::Error> {
    let mut offset = 0;
    while let Some((token, rem)) = ptr.split_front() {
        let tok_len = token.encoded().len();
        ptr = rem;

        value = if value.is_array() {
            let items = value.data.as_mut_vec().unwrap();
            let idx = token
                .to_index()
                .map_err(|source| ResolveError::FailedToParseIndex { offset, source })?
                .for_len(items.len())
                .map_err(|source| ResolveError::OutOfBounds { offset, source })?;
            &mut items[idx]
        } else if value.is_hash() {
            let items = value.data.as_mut_hash().unwrap();
            let token = token.decoded().to_string();
            let key = MarkedYaml::from_bare_yaml(saphyr::Yaml::String(token));
            &mut items[&key]
        } else {
            // return Err(ResolveError::Unreachable { offset }.).;
            bail!("This totally failed!");
        };
        offset += 1 + tok_len;
    }
    Ok(value)
}

fn apply_patch(patches: &json_patch::Patch, doc: &mut MarkedYaml) -> Result<(), Error> {
    for p in patches.iter() {
        match p {
            PatchOperation::Replace(r) => {
                if let Ok(v) = resolve_mut2(doc, &r.path) {
                    let the_yaml = serde_yaml::to_string(&r.value)
                        .expect("should turn patch value into yaml string");
                    let replacement = MarkedYaml::load_from_str(the_yaml.as_str())
                        .expect("valid yaml?")
                        .remove(0);
                    *v = replacement;
                } else {
                    return Err(Error::ValueNotFoundAtPath);
                }
            }
            PatchOperation::Add(a) => {
                if let Some((path, field)) = a.path.split_back() {
                    if let Ok(v) = resolve_mut2(doc, path) {
                        if let Some(m) = v.data.as_mut_hash() {
                            let the_yaml = serde_yaml::to_string(&a.value)
                                .expect("should turn patch value into yaml string");
                            let replacement = MarkedYaml::load_from_str(the_yaml.as_str())
                                .expect("valid yaml?")
                                .remove(0);
                            let key = MarkedYaml::from_bare_yaml(Yaml::String(field.to_string()));
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

fn document_matches(document_like: &serde_yaml::Value, actual_doc: &MarkedYaml) -> bool {
    match (document_like, &actual_doc.data) {
        (serde_yaml::Value::Null, YamlData::Null) => true,
        (serde_yaml::Value::Bool(a), YamlData::Boolean(b)) => a == b,
        (serde_yaml::Value::Number(n), YamlData::Integer(b)) if n.is_i64() => {
            n.as_i64().unwrap() == *b
        }
        (serde_yaml::Value::Number(n), YamlData::Real(b)) if n.is_f64() => {
            let a = n.as_f64().unwrap();
            let Ok(b) = b.parse::<f64>() else {
                return false;
            };
            a == b
        }
        (serde_yaml::Value::String(a), YamlData::String(b)) => a == b,
        (serde_yaml::Value::Sequence(required), YamlData::Array(available)) => {
            for (r, a) in required.iter().zip(available.iter()) {
                if !document_matches(r, a) {
                    return false;
                }
            }
            true
        }
        (serde_yaml::Value::Mapping(required), YamlData::Hash(available)) => {
            for (key, value) in required {
                let Some(raw_val) = key.as_str() else {
                    return false;
                };
                let Some(other_value) = available.get(&MarkedYaml::from_bare_yaml(Yaml::String(
                    raw_val.to_string(),
                ))) else {
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

    use crate::{YamlSource, first_node, last_line_in_node, snippet::Line};

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

        let _pp: PrePatch = serde_yaml::from_str(raw).unwrap();
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
        let pp: PrePatch = serde_yaml::from_str(indoc! {r#"
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
        let pp: PrePatch = serde_yaml::from_str(indoc! {r#"
            name: Add the namespace everywhere
            patches:
              - op: add
                path: "/metadata/namespace"
                value: "core"
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
        let n = saphyr::MarkedYaml::load_from_str(raw)
            .expect("Bla bla something reading docs into the system");

        n.into_iter()
            .enumerate()
            .map(|(index, yaml)| YamlSource {
                file: camino::Utf8PathBuf::new(),
                yaml,
                content: raw.to_string(), // TODO: hmmm...
                index,
                // TODO: What goes here?
                first_line: Line::try_from(1).unwrap(),
                last_line: Line::try_from(10).unwrap(),
            })
            .collect()
    }

    pub fn serialize(docs: &[YamlSource]) -> String {
        let mut out_str = String::new();
        let mut emitter = YamlEmitter::new(&mut out_str);
        for doc in docs {
            emitter.dump(&doc.yaml).unwrap();
        }
        out_str
    }
}
