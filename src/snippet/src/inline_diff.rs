use similar::{ChangeTag, TextDiff};

/// A part of an inline diff, with text and whether it should be emphasized (highlighted).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlinePart {
    pub text: String,
    pub emphasized: bool,
}

/// Compute character-level inline diff between two strings.
/// Returns (left_parts, right_parts) where:
/// - left_parts contains Delete + Equal chunks
/// - right_parts contains Insert + Equal chunks
pub(crate) fn compute_inline_diff(left: &str, right: &str) -> (Vec<InlinePart>, Vec<InlinePart>) {
    let diff = TextDiff::from_chars(left, right);

    let mut left_parts = Vec::new();
    let mut right_parts = Vec::new();

    // Collect consecutive changes of the same type into single parts
    let mut current_left_text = String::new();
    let mut current_left_emphasized = false;
    let mut current_right_text = String::new();
    let mut current_right_emphasized = false;

    for change in diff.iter_all_changes() {
        // Get the actual character (change.value() returns &str for the char)
        let ch = change.value();
        if ch.is_empty() {
            continue;
        }

        // Makes longer runs emphasised/non-emphasised text.
        match change.tag() {
            ChangeTag::Equal => {
                // Flush any pending emphasized parts before adding equal parts
                if !current_left_text.is_empty() && current_left_emphasized {
                    left_parts.push(InlinePart {
                        text: std::mem::take(&mut current_left_text),
                        emphasized: true,
                    });
                }
                if !current_right_text.is_empty() && current_right_emphasized {
                    right_parts.push(InlinePart {
                        text: std::mem::take(&mut current_right_text),
                        emphasized: true,
                    });
                }

                // Append to current unchanged parts
                current_left_text.push_str(ch);
                current_left_emphasized = false;
                current_right_text.push_str(ch);
                current_right_emphasized = false;
            }
            ChangeTag::Delete => {
                // Flush left unchanged part if we're switching to emphasized
                if !current_left_text.is_empty() && !current_left_emphasized {
                    left_parts.push(InlinePart {
                        text: std::mem::take(&mut current_left_text),
                        emphasized: false,
                    });
                }
                current_left_text.push_str(ch);
                current_left_emphasized = true;
            }
            ChangeTag::Insert => {
                // Flush right unchanged part if we're switching to emphasized
                if !current_right_text.is_empty() && !current_right_emphasized {
                    right_parts.push(InlinePart {
                        text: std::mem::take(&mut current_right_text),
                        emphasized: false,
                    });
                }
                current_right_text.push_str(ch);
                current_right_emphasized = true;
            }
        }
    }

    // Flush any remaining parts
    if !current_left_text.is_empty() {
        left_parts.push(InlinePart {
            text: current_left_text,
            emphasized: current_left_emphasized,
        });
    }
    if !current_right_text.is_empty() {
        right_parts.push(InlinePart {
            text: current_right_text,
            emphasized: current_right_emphasized,
        });
    }

    (left_parts, right_parts)
}

/// Extract the YAML prefix (indentation + key + colon + space) from a line.
/// For "  image: registry.k8s.io/kube-proxy:v1.33.1", returns "  image: "
/// For "    - value", returns "    - "
pub(crate) fn extract_yaml_prefix(line: &str) -> &str {
    // Find the position after ": " (for key-value pairs)
    if let Some(pos) = line.find(": ") {
        return &line[..pos + 2];
    }
    // For array items like "  - value", find position after "- "
    if let Some(pos) = line.find("- ") {
        return &line[..pos + 2];
    }
    // Fallback: return empty prefix (the whole line is the value)
    ""
}

#[cfg(test)]
mod tests {
    use crate::inline_diff::InlinePart;

    use super::compute_inline_diff;

    /// Reconstructs the parts putting `[...]` around emphasised parts
    fn reconstruct(parts: &[InlinePart]) -> String {
        parts
            .iter()
            .map(|p| {
                let text = p.text.as_str();
                if p.emphasized {
                    format!("[{text}]")
                } else {
                    text.to_string()
                }
            })
            .collect()
    }

    #[test]
    fn version_change_highlights_only_differing_parts() {
        // v1.33.1 -> v1.35.0: character-level diff
        let (left_parts, right_parts) = compute_inline_diff("v1.34.7-build1", "v1.35.0-build1");

        let left_reconstructed = reconstruct(&left_parts);
        let right_reconstructed = reconstruct(&right_parts);

        assert_eq!(left_reconstructed, "v1.3[4].[7]-build1");
        assert_eq!(right_reconstructed, "v1.3[5].[0]-build1");
    }

    #[test]
    fn partial_string_change() {
        // "Hello World" -> "Hello Universe": only "World"/"Universe" differ
        let (left_parts, right_parts) = compute_inline_diff("Hello World", "Hello Universe");

        let left_reconstructed = reconstruct(&left_parts);
        let right_reconstructed = reconstruct(&right_parts);

        assert_eq!(left_reconstructed, "Hello [Wo]r[ld]");
        assert_eq!(right_reconstructed, "Hello [Unive]r[se]");
    }

    #[test]
    fn completely_different_strings() {
        let (left_parts, right_parts) = compute_inline_diff("abc", "xyz");

        let left_reconstructed = reconstruct(&left_parts);
        let right_reconstructed = reconstruct(&right_parts);

        assert_eq!(left_reconstructed, "[abc]");
        assert_eq!(right_reconstructed, "[xyz]");
    }

    #[test]
    fn identical_strings_no_emphasis() {
        let (left_parts, right_parts) = compute_inline_diff("same", "same");

        let left_reconstructed = reconstruct(&left_parts);
        let right_reconstructed = reconstruct(&right_parts);

        assert_eq!(left_reconstructed, "same");
        assert_eq!(right_reconstructed, "same");
    }

    #[test]
    fn full_image_path_change() {
        // Real-world example: image tag change
        let left = "registry.k8s.io/kube-proxy:v1.33.1";
        let right = "registry.k8s.io/kube-proxy:v1.35.0";

        let (left_parts, right_parts) = compute_inline_diff(left, right);

        let left_reconstructed = reconstruct(&left_parts);
        let right_reconstructed = reconstruct(&right_parts);

        assert_eq!(left_reconstructed, "registry.k8s.io/kube-proxy:v1.3[3].[1]");
        assert_eq!(
            right_reconstructed,
            "registry.k8s.io/kube-proxy:v1.3[5].[0]"
        );
    }
}
