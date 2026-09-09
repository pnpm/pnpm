---
"pacquet": patch
---

Commands run from a subdirectory of a project now act on the project, as they do on pnpm 11. `pnpm run` reported no manifest at all [#14664](https://github.com/pnpm/pnpm/issues/14664). `pnpm bin` printed a `node_modules/.bin` path under the current directory, which does not exist [#14622](https://github.com/pnpm/pnpm/issues/14622). A subdirectory holding only a `Cargo.toml` or a `pyproject.toml` is itself the project for the commands that install dependencies, so adding a crate there edits the nearest `Cargo.toml`. The commands that only read `package.json` or `node_modules` carry on to the npm project around it, `pnpm run`, `pnpm exec`, `pnpm bin`, `pnpm root`, `pnpm pkg`, `pnpm set-script`, and `pnpm clean` among them. `pnpm init` still creates its `package.json` in the current directory, and `pnpm exec` still runs its command there.
