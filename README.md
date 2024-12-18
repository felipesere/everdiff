# Everdiff

## Features

### Watch mode

When you need to keep re-running `everdiff` as you evolve a set of documents, its nice to hit `--watch` to let it watch all the input files and re-run when needed.

### Prepatching

When migrating Kubernetes manifest from one tool to another, conventions sometimes clash.
For example, a team may have used `Kustomize` and named their `NetworkPolicy` as `thing-netpol`, whereas the `Helm` chart
that their company is migrating towards just calls is `thing`, as `-netpol` is implied by the `kind`.
When running `everdiff` with the `--kubernetes` flag, it will look at some fields such as that `name` to identify
which documents should be compared. If the name is different between the netpols that are _semantically_ the same,
it will show up as an addition and a removal.

With `prepatches` we can apply changes to documents _before_ they get matched up and diffed.
This helps reduce the number of visible changes and helps narrow down what actually matters.
By tracking these `prepatches` in a config file we make sure we can document what changes we 
apply and we can carry them between calls (e.g. when first diffing `development` and then also diffing `production`).

A config file with prepates looks like this:

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

`documentLike` shows a snippet of the document that should match. In this case it will only match `NetworkPolicy` resources that are named `flux-engine-steam`
The `patches` are JSONPatches with the limitation that we only support `op: replace` and `op: add` at the moment.


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
