---
"pacquet": patch
---

`pnpm pack` and `pnpm publish` no longer let workspace-root `.gitignore` / `.npmignore` rules exclude files matched by the package manifest's `files` allowlist. Workspace packages whose build output is gitignored at the workspace root (for example a compiled `lib/` directory listed in `files`) were published with almost all payload files missing [#13164](https://github.com/pnpm/pnpm/issues/13164).
