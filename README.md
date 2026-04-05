# Everdiff

A semantic diff tool for YAML documents that understands structure, not just text.

## Installation

### Homebrew

```sh
brew install felipesere/tap/everdiff
```

### From source

```sh
cargo install --path .
```

## Usage

```
everdiff [-k] [-m] [-i=PATH]... [-w] [-B=NUMBER] [-A=NUMBER] [-C=NUMBER] [-v]... LEFT RIGHT

Available positional items:
    LEFT                Left file to compare
    RIGHT               Right file to compare

Available options:
    -k, --kubernetes    Use Kubernetes comparison
    -m, --ignore-moved  Don't show changes for moved elements
    -i, --ignore-changes=PATH  Paths to ignore when comparing
    -B, --lines-before=NUMBER  Number of context lines to show before each change (default: 5)
    -A, --lines-after=NUMBER   Number of context lines to show after each change (default: 5)
    -C, --lines-context=NUMBER Number of context lines before and after each change (overrides -A and -B)
    -v, --verbose       Increase verbosity level (can be repeated)
    -h, --help          Prints help information
    --version           Show version information
```

## Examples

### Basic comparison

Compare two YAML files:

```sh
everdiff before.yaml after.yaml
```

Or using shell brace expansion:

```sh
everdiff {before,after}.yaml
```

Given these two files:

```yaml
# before.yaml
person:
  name: Steve E. Anderson
  age: 12
---
pet:
  kind: cat
  age: 7
```

```yaml
# after.yaml
person:
  name: Steven Anderson
  location:
    street: 1 Kentish Street
    postcode: KS87JJ
  age: 34
---
pet:
  kind: dog
  age: 9
```

The output shows semantic changes:

![everdiff showing the differnce between two YAML files, using colors and alignment to emphasise changes](assets/before-after.png)


### Kubernetes mode

When comparing Kubernetes manifests, use `--kubernetes` to match documents by their GVK (Group/Version/Kind) and name:

```sh
everdiff --kubernetes before.yaml after.yaml
```

Documents are identified by `apiVersion`, `kind`, and `metadata.name` rather than by position:

```
Changed document:
    ╭───────────────┬───────────────────╮
    │ api_version   ┆ apps/v1           │
    ├╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┤
    │ kind          ┆ Deployment        │
    ├╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┼╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┤
    │ metadata.name ┆ flux-engine-steam │
    ╰───────────────┴───────────────────╯
Changed: .spec.replicas:
│  14 │ spec:                           │  15 │ spec:
│  15 │   replicas: 3                   │  16 │   replicas: 4
```

### Ignoring moved elements

When array elements are reordered, `everdiff` reports them as "Moved". Use `--ignore-moved` to hide these:

```sh
everdiff --kubernetes --ignore-moved before.yaml after.yaml
```

### Controlling context lines

By default, `everdiff` shows 5 lines of context before and after each change. Use `-A`, `-B`, and `-C` to adjust this, similar to `diff` and `grep`:

```sh
# Show 3 lines before and 3 lines after each change
everdiff -C 3 before.yaml after.yaml

# Show 1 line before and 10 lines after each change
everdiff -B 1 -A 10 before.yaml after.yaml

# Show no context at all
everdiff -C 0 before.yaml after.yaml
```

`-C` sets both before and after to the same value and cannot be combined with `-A` or `-B`.

### Ignoring specific paths

Use `--ignore-changes` to exclude certain paths from the diff:

```sh
everdiff before.yaml after.yaml \
    --ignore-changes '.metadata.annotations' \
    --ignore-changes '.spec.replicas'
```

Path patterns support:
- Exact paths: `.metadata.name`
- Array indices: `.spec.containers[0].image`
- Wildcards: `.metadata.labels.*`

## License

MIT
