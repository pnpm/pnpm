---
"pnpm": major
"@pnpm/plugin-commands-installation": major
---

Breaking changes to `pnpm link`:

- `pnpm link <pkg-name>` no longer resolves packages from the global store. Only relative or absolute paths are accepted. For example, use `pnpm link ./foo` instead of `pnpm link foo`.
- `pnpm link --global` is removed. Use `pnpm add -g .` to register a local package's bins globally.
- `pnpm link` (no arguments) is removed. Use `pnpm link <dir>` with an explicit path instead.
