---
"pacquet": patch
---

Commands run from a subdirectory of a project now act on the nearest ancestor directory that has a manifest, as they do on pnpm 11. `pnpm run` reported no manifest at all [#14664](https://github.com/pnpm/pnpm/issues/14664), and `pnpm bin` printed a `node_modules/.bin` path under the current directory, which does not exist [#14622](https://github.com/pnpm/pnpm/issues/14622). A subdirectory that holds only a `Cargo.toml` or a `pyproject.toml` bounds that walk for the commands that install dependencies, so `pnpm add crate:serde` there edits the nearest `Cargo.toml`, while `pnpm run`, `pnpm exec`, `pnpm bin`, and `pnpm root` carry on to the npm project around it. `pnpm init` still creates its `package.json` in the current directory, and `pnpm exec` still runs its command there.
