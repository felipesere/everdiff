# Everdiff Development Notes

## Code Quality Assessment

### Strengths
- Clean separation of concerns with well-defined modules (`diff`, `multidoc`, `identifier`, etc.)
- Sophisticated dynamic array ordering with difference matrix optimization for detecting moved elements
- Good CLI interface with file watching, Kubernetes-specific comparison modes, and flexible ignore patterns
- Comprehensive test coverage with realistic examples like NetworkPolicy test cases
- The `Context` struct for diff configuration is elegant and extensible

### Areas for Improvement

#### Error Handling
- Replace `anyhow` with structured error types for better error handling and user experience
- Inconsistent error handling patterns throughout the codebase
- Some functions use `unwrap()` where proper error propagation would be better

#### Performance
- Reading entire files into memory could be problematic for large YAML files
- The difference matrix approach scales O(n²) for large arrays - consider using LCS or similar algorithms
- Break down complex `minimize_differences` function into smaller, more manageable pieces

#### Code Organization
- Some magic numbers and hardcoded values need to be extracted into named constants
- Limited output formatting options beyond side-by-side mode

## Refactoring Suggestions for `snippet.rs`

### Priority: High

#### 1. Break Down Large Functions
The `render_change` function (lines 330-549) is 219 lines long and handles too many responsibilities:

```rust
// Current: One massive function doing everything
fn render_change(...) -> String {
    // 219 lines of mixed concerns
}

// Suggested: Break into focused functions
fn calculate_snippet_bounds(...) -> SnippetBounds { }
fn render_primary_side(...) -> Vec<String> { }
fn calculate_gap_positions(...) -> GapInfo { }
fn render_secondary_side_with_gap(...) -> Vec<String> { }
fn combine_sides(...) -> String { }
```

### Priority: Medium

#### 2. Extract Magic Numbers
Replace scattered magic numbers with named constants:

```rust
const DEFAULT_CONTEXT_SIZE: usize = 5;
const LINE_WIDGET_WIDTH: usize = 4;
const PADDING_ADJUSTMENT: u16 = 16;
const GAP_ADJUSTMENT_FOR_MAPPINGS: usize = 1;
const RANDOM_PADDING: usize = 6; // The "horrid 6" mentioned in comments
```

#### 3. Create Configuration Struct
Replace the many parameters with a configuration struct:

```rust
struct RenderConfig {
    max_width: u16,
    context_size: usize,
    color: Color,
    side_by_side: bool,
}

// Instead of: render_change(path, yaml, left_doc, right_doc, max_width, color, change_type)
// Use: render_change(path, yaml, left_doc, right_doc, config, change_type)
```

#### 4. Extract Gap Calculation Logic
The gap calculation logic (lines 427-476) is complex and deserves its own functions:

```rust
struct GapInfo {
    start: Line,
    end: Line,
    size: usize,
}

fn calculate_gap_bounds(changed_yaml: &MarkedYamlOwned, ...) -> GapInfo { }
fn find_surrounding_nodes(...) -> (Option<Path>, Option<Path>) { }
fn estimate_gap_size(...) -> usize { }
```

#### 5. Simplify Line Arithmetic
The `Line` struct has complex arithmetic operations that could be simplified:

```rust
// Current: Multiple Add/Sub implementations with different behavior
impl Add<usize> for Line { }
impl Add<i32> for Line { }
impl Sub<usize> for Line { }
// ... etc

// Consider: LineRange struct for span operations
struct LineRange {
    start: Line,
    end: Line,
}
```

### Priority: Low

#### 6. Create Unified Renderer
Instead of separate `render_added`, `render_removal`, `render_difference` functions, create a unified approach:

```rust
struct SnippetRenderer {
    config: RenderConfig,
}

impl SnippetRenderer {
    fn render_addition(&self, ...) -> String { }
    fn render_removal(&self, ...) -> String { }
    fn render_change(&self, ...) -> String { }
}
```

#### 7. Reduce Code Duplication
Lines 503-509 and 526-532 have very similar logic for formatting lines. Extract into a shared function:

```rust
fn format_snippet_line(line_nr: Line, content: &str, style: Style, max_width: usize) -> String {
    let line = content.style(style).to_string();
    let extras = line.len() - ansi_width(&line);
    let line_nr = LineWidget::from(line_nr);
    format!("{line_nr}│ {line:<width$}", width = max_width + extras)
}
```

## Next Steps

1. **Immediate**: Remove debug error log at `main.rs:95` ✅
2. **Short-term**: Fix typos in comments ✅
3. **Medium-term**: Implement structured error types
4. **Long-term**: Refactor `snippet.rs` according to suggestions above

## Technical Debt

- The comment "this is getting stupid... I need to track these better..." at `diff.rs:200` indicates awareness of complexity issues
- TODO comment at `snippet.rs:117` about "gross ±1 math" suggests Line concept needs refinement
- Magic number "6" described as "horrid" in comment at line 709 needs proper solution

## Architecture Considerations

The codebase shows good Rust practices overall but would benefit from:
- More consistent error handling patterns
- Better separation of rendering concerns
- Performance optimizations for large files
- More flexible output formatting options