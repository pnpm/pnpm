---
"@pnpm/plugin-commands-config": major
"pnpm": major
---

The naming cases of top-level config keys have been changed for `pnpm config list [--json]` and `pnpm config get [--json]` (without argument).
The specifics depend on the appearance of `--json` and the classification of the top-level keys.

`pnpm config list` and `pnpm config get` (without `--json`) imitate an rc file (INI) with best effort.
They would show rc options in kebab-case, workspace-specific settings in camelCase (since they cannot
be defined by an rc file).

`pnpm config list --json` and `pnpm config get --json` imitate the keys in `pnpm-workspace.yaml` with best effort.
They would show both rc options and workspace-specific settings in camelCase.

Exception: Keys that start with `@` or `//` would be preserved (their cases don't change).
