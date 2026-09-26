## 1100.0.1

### Patch Changes

- Wildcards in negated `packages` patterns of `pnpm-workspace.yaml` now match directories whose names start with a dot. For example, `!packages/**` now also excludes `packages/.dev/tool` when another pattern includes `.dev` explicitly.
