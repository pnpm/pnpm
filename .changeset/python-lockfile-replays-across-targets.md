---
"pacquet": patch
---

`pnpm install --frozen-lockfile` now replays a `pylock.toml` on any Python target that can install it. Previously the lockfile was reused only for the exact marker environment and wheel tag order that produced it, so a kernel update alone invalidated it. A lockfile is now reused when the project's requirements, index and `requires-python` are unchanged, every pinned wheel carries tags the interpreter accepts, and the locked packages are exactly what the interpreter's markers select [#14843](https://github.com/pnpm/pnpm/issues/14843).

The `environments` marker written to `pylock.toml` now names only the interpreter version and the marker variables the locked dependency graph reads. When the lockfile is not frozen and its locked graph no longer matches the target, `pnpm install` warns and resolves the project again.
