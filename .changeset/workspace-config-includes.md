---
"pacquet": minor
---

`pnpm-workspace.yaml` can now delegate settings to other YAML files and to config dependencies through a new `include` field. Each source may define only the settings the root manifest delegates to it, and every setting has one owner. Commands that change settings write each one to the file that owns it. A relative path in an included file is relative to that file. See [Splitting Settings Across Files](https://pnpm.io/config-includes).

A config dependency supplies its settings through the YAML file named in the `pnpm.config` field of its `package.json`. The new `bootConfig` setting sets the release-age and trust policy pnpm uses to install config dependencies and the pnpm version from `packageManager`.
