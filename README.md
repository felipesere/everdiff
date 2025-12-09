# Everdiff

A semantic diff tool for YAML documents that understands structure, not just text.

## Installation

```sh
cargo install --path .
```

## Usage

```
everdiff [-s] [-k] [-m] [-i=PATH]... [-w] [-v]... -l=PATH... -r=PATH...

Available options:
    -s, --side-by-side  Render differences side-by-side
    -k, --kubernetes    Use Kubernetes comparison
    -m, --ignore-moved  Don't show changes for moved elements
    -i, --ignore-changes=PATH  Paths to ignore when comparing
    -w, --watch         Watch the `left` and `right` files for changes and re-run
    -v, --verbose       Increase verbosity level (can be repeated)
    -l, --left=PATH     Left file(s) to compare
    -r, --right=PATH    Right file(s) to compare
    -h, --help          Prints help information
```

## Examples

### Basic comparison

Compare two YAML files:

```sh
everdiff --left before.yaml --right after.yaml
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

```
Changed document:
    ╭─────┬───╮
    │ idx ┆ 0 │
    ╰─────┴───╯
Changed: .person.name:
│   1 │ person:                         │   1 │ person:
│   2 │   name: Steve E. Anderson       │   2 │   name: Steven Anderson
│   3 │   age: 12                       │   3 │   location:
│                                       │   4 │     street: 1 Kentish Street
│                                       │   5 │     postcode: KS87JJ
│                                       │   6 │   age: 34

Added: .person.location:
│   1 │ person:                         │   1 │ person:
│   2 │   name: Steve E. Anderson       │   2 │   name: Steven Anderson
│     │                                 │   3 │   location:
│     │                                 │   4 │     street: 1 Kentish Street
│     │                                 │   5 │     postcode: KS87JJ
│   3 │   age: 12                       │   6 │   age: 34
```

### Kubernetes mode

When comparing Kubernetes manifests, use `--kubernetes` to match documents by their GVK (Group/Version/Kind) and name:

```sh
everdiff --kubernetes --left before.yaml --right after.yaml
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
everdiff --kubernetes --ignore-moved --left before.yaml --right after.yaml
```

### Ignoring specific paths

Use `--ignore-changes` to exclude certain paths from the diff:

```sh
everdiff --left before.yaml --right after.yaml \
    --ignore-changes '.metadata.annotations' \
    --ignore-changes '.spec.replicas'
```

Path patterns support:
- Exact paths: `.metadata.name`
- Array indices: `.spec.containers[0].image`
- Wildcards: `.metadata.labels.*`

### Multiple input files

Compare multiple files at once:

```sh
everdiff --kubernetes \
    --left deployment.yaml service.yaml \
    --right new-deployment.yaml new-service.yaml
```

## Features

### Watch mode

When you need to keep re-running `everdiff` as you evolve a set of documents, use `--watch` to let it watch all the input files and re-run when needed:

```sh
everdiff --watch --left before.yaml --right after.yaml
```

### Prepatching

When migrating Kubernetes manifests from one tool to another, conventions sometimes clash.
For example, a team may have used `Kustomize` and named their `NetworkPolicy` as `thing-netpol`, whereas the `Helm` chart
that their company is migrating towards just calls it `thing`, as `-netpol` is implied by the `kind`.
When running `everdiff` with the `--kubernetes` flag, it will look at some fields such as that `name` to identify
which documents should be compared. If the name is different between the netpols that are _semantically_ the same,
it will show up as an addition and a removal.

With `prepatches` we can apply changes to documents _before_ they get matched up and diffed.
This helps reduce the number of visible changes and helps narrow down what actually matters.
By tracking these `prepatches` in a config file we make sure we can document what changes we
apply and we can carry them between calls (e.g. when first diffing `development` and then also diffing `production`).

A config file with prepatches (`everdiff.config.yaml`) looks like this:

```yaml
prepatches:
  - name: rename network policy to match chart convention
    documentLike:
      kind: NetworkPolicy
      metadata:
        name: flux-engine-steam
    patches:
      - op: replace
        path: /metadata/name
        value: flux
```

`documentLike` shows a snippet of the document that should match. In this case it will only match `NetworkPolicy` resources that are named `flux-engine-steam`.
The `patches` are JSONPatches with the limitation that we only support `op: replace` and `op: add` at the moment.

<details>
  <summary>Example use case</summary>

Assume the following documents exists:
```yaml
# before.netpol.yaml
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: flux-netpol
  namespace: some
spec:
  podSelector:
    matchLabels:
      app: flux-engine-steam
  policyTypes:
    - Egress
  egress:
    - to:
        - namespaceSelector:
            matchLabels:
              name: opentelemetry-operator-system
      ports:
        - port: 13133
```
and
```yaml
# after.netpol.yaml
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata:
  name: flux
  namespace: some
spec:
  podSelector:
    matchLabels:
      app: flux-engine-steam
  policyTypes:
    - Egress
  egress:
    - to:
        - namespaceSelector:
            matchLabels:
              name: opentelemetry-operator-system
      ports:
        - port: 13133
        - port: 8080
```
Just running `everdiff --kubernetes --left before.netpol.yaml --right after.netpol.yaml` will say that there is a document added and one removed:

```sh
Missing document:
    api_version → networking.k8s.io/v1
    kind → NetworkPolicy
    metadata.name → flux-engine-steam

Additional document:
    api_version → networking.k8s.io/v1
    kind → NetworkPolicy
    metadata.name → flux
```

We can see the `metadata.name` changes. But we know that they are semantically the same so we'd like to see if there
are any meaningful differences.

So we run the command again but with the following config in `everdiff.config.yaml`:

```yaml
prepatches:
  - name: rename network policy to match chart convention
    documentLike:
      kind: NetworkPolicy
      metadata:
        name: flux-engine-steam
    patches:
      - op: replace
        path: /metadata/name
        value: flux
```
And now the output is different and more interesting:
```sh
Loaded configuration...
Changed document:
    api_version → networking.k8s.io/v1
    kind → NetworkPolicy
    metadata.name → flux

Added: .spec.egress[0].ports[1]:
    port: 8080
```

After lining up the names of the netpol, we see that the real change is the addition of port `8080` to the first egress rule.
</details>

## TODO

- [x] Watch mode, where it keeps re-running for the files every time it detects a `write`
- [x] Pre-Patch resources to make the diff smaller/more accurate
  - [x] for example, if working with K8S resources we may want to change the name of a resource from `name: service-netpol` to `name: service`
    before looking for changes
- [ ] Ignored differences: Have an interactive way to say "this change does not matter"
- [ ] Persist both ignored differences and pre-patches as file that can be shared
- [ ] Context-aware ways to perform diffs
  - [ ] K8S: things with the same `name` will be expected to be the same, particularly for entire resources
  - [ ] Lax: order in arrays does not matter, minimize changes
  - [ ] Strict: any change is a full change
