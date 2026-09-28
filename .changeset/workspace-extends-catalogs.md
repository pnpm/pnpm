---
"pacquet": minor
---

`pnpm-workspace.yaml` accepts an `extends` field that pulls in the catalogs of other workspace manifests, so several workspaces can share one set of catalogs [pnpm/pnpm#12475](https://github.com/pnpm/pnpm/pull/12475):

```yaml
extends:
  - ../shared-config          # a directory holding a pnpm-workspace.yaml
  - ./catalogs/react.yaml     # or the path of the manifest itself
  - packages/*                # or a glob; matches without a manifest are skipped
catalog:
  react: ^19.0.0              # entries declared here win over inherited ones
```

Catalogs merge entry by entry. A manifest listed later wins over one listed earlier, and glob matches count in path order. An extended manifest can extend others in turn. A reference that forms a cycle, or that names a directory without a `pnpm-workspace.yaml`, fails the install. `extends` only affects catalogs, and only in the manifest that acts as the workspace root. Commands that update catalogs write to the root manifest and never copy inherited entries into it.
