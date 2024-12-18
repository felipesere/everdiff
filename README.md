# Everdiff

## Features (that don't exit yet)

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
