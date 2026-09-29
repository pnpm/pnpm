---
"pacquet": minor
---

Catalogs can now be shared between workspace manifests, and a project that keeps its own lockfile can have catalogs of its own [pnpm/pnpm#12475](https://github.com/pnpm/pnpm/pull/12475).

`pnpm-workspace.yaml` accepts an `extends` field that pulls in the catalogs of other workspace manifests:

```yaml
extends:
  - ../shared-config          # a directory holding a pnpm-workspace.yaml
  - ./catalogs/react.yaml     # or the path of the manifest itself
  - packages/*                # or a glob; matches without a manifest are skipped
catalog:
  react: ^19.0.0              # entries declared here win over inherited ones
```

Catalogs merge entry by entry. A manifest listed later wins over one listed earlier, and glob matches count in path order. An extended manifest can extend others in turn, and local paths in its entries stay relative to it. A reference that forms a cycle, or that names a directory without a `pnpm-workspace.yaml`, fails the install.

With `sharedWorkspaceLockfile: false`, a project that has a `pnpm-workspace.yaml` of its own now resolves `catalog:` against that manifest's catalogs (including whatever it `extends`), instead of the workspace root's. This is for projects that are also installed on their own, such as git submodules. Take a workspace that pins a dependency for every project, and one submodule that has to stay on an older major:

```yaml
# pnpm-workspace.yaml
packages:
  - apps/*
sharedWorkspaceLockfile: false
catalog:
  react: ^19.0.0
```

```yaml
# apps/legacy/pnpm-workspace.yaml (a git submodule)
catalog:
  react: ^17.0.0
```

`apps/legacy` installs `react@17` both when the workspace installs it and when it is installed on its own, so both installs write the same `apps/legacy/pnpm-lock.yaml`, and `--frozen-lockfile` passes in either place. Catalog entries that `pnpm add` or `pnpm update` saves for that project go to its own `pnpm-workspace.yaml`, and `pnpm pack` and `pnpm publish` replace `catalog:` with its own entries. Every other setting in a project's own `pnpm-workspace.yaml` still does not apply, and pnpm names the ignored ones in a warning. With a shared lockfile, every project keeps using the workspace root's catalogs.
