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
    use super::compute_inline_diff;

    #[test]
    fn version_change_highlights_only_differing_parts() {
        // v1.33.1 -> v1.35.0: character-level diff
        let (left_parts, right_parts) = compute_inline_diff("v1.33.1", "v1.35.0");

        // Verify the common prefix is unchanged
        let left_unchanged: String = left_parts
            .iter()
            .filter(|p| !p.emphasized)
            .map(|p| p.text.as_str())
            .collect();
        assert!(left_unchanged.contains("v1.3"));

        // Verify emphasized parts exist and are smaller than the full string
        let left_emphasized: String = left_parts
            .iter()
            .filter(|p| p.emphasized)
            .map(|p| p.text.as_str())
            .collect();
        let right_emphasized: String = right_parts
            .iter()
            .filter(|p| p.emphasized)
            .map(|p| p.text.as_str())
            .collect();

        assert!(!left_emphasized.is_empty());
        assert!(!right_emphasized.is_empty());
        assert!(left_emphasized.len() < "v1.33.1".len());
        assert!(right_emphasized.len() < "v1.35.0".len());

        // When concatenated, parts should reconstruct the original strings
        let left_reconstructed: String = left_parts.iter().map(|p| p.text.as_str()).collect();
        let right_reconstructed: String = right_parts.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(left_reconstructed, "v1.33.1");
        assert_eq!(right_reconstructed, "v1.35.0");
    }

    #[test]
    fn partial_string_change() {
        // "Hello World" -> "Hello Universe": only "World"/"Universe" differ
        let (left_parts, right_parts) = compute_inline_diff("Hello World", "Hello Universe");

        // The common prefix "Hello " should be unchanged
        // "World" vs "Universe" will show up as changed parts
        let left_emphasized: Vec<_> = left_parts.iter().filter(|p| p.emphasized).collect();
        let right_emphasized: Vec<_> = right_parts.iter().filter(|p| p.emphasized).collect();

        // There should be some emphasized parts on each side
        assert!(!left_emphasized.is_empty());
        assert!(!right_emphasized.is_empty());

        // The unchanged parts should include "Hello "
        let left_unchanged_text: String = left_parts
            .iter()
            .filter(|p| !p.emphasized)
            .map(|p| p.text.as_str())
            .collect();
        assert!(left_unchanged_text.contains("Hello "));
    }

    #[test]
    fn completely_different_strings() {
        let (left_parts, right_parts) = compute_inline_diff("abc", "xyz");

        // Everything should be emphasized since nothing matches
        let left_emphasized: Vec<_> = left_parts.iter().filter(|p| p.emphasized).collect();
        let right_emphasized: Vec<_> = right_parts.iter().filter(|p| p.emphasized).collect();

        assert!(!left_emphasized.is_empty());
        assert!(!right_emphasized.is_empty());
    }

    #[test]
    fn identical_strings_no_emphasis() {
        let (left_parts, right_parts) = compute_inline_diff("same", "same");

        // Nothing should be emphasized
        let left_emphasized: Vec<_> = left_parts.iter().filter(|p| p.emphasized).collect();
        let right_emphasized: Vec<_> = right_parts.iter().filter(|p| p.emphasized).collect();

        assert!(left_emphasized.is_empty());
        assert!(right_emphasized.is_empty());

        // The full text should be present as unchanged
        let left_text: String = left_parts.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(left_text, "same");
    }

    #[test]
    fn full_image_path_change() {
        // Real-world example: image tag change
        let left = "registry.k8s.io/kube-proxy:v1.33.1";
        let right = "registry.k8s.io/kube-proxy:v1.35.0";

        let (left_parts, right_parts) = compute_inline_diff(left, right);

        // The common prefix "registry.k8s.io/kube-proxy:v1." should be unchanged
        let left_unchanged_text: String = left_parts
            .iter()
            .filter(|p| !p.emphasized)
            .map(|p| p.text.as_str())
            .collect();

        assert!(left_unchanged_text.contains("registry.k8s.io/kube-proxy:v1."));

        // Only the version-specific parts should be emphasized
        let left_emphasized_text: String = left_parts
            .iter()
            .filter(|p| p.emphasized)
            .map(|p| p.text.as_str())
            .collect();
        let right_emphasized_text: String = right_parts
            .iter()
            .filter(|p| p.emphasized)
            .map(|p| p.text.as_str())
            .collect();

        // The emphasized parts should be the differing version numbers
        assert!(left_emphasized_text.len() < left.len());
        assert!(right_emphasized_text.len() < right.len());
    }
}
