---
"pacquet": minor
---

**Breaking change.** A filtered install now also installs the dependencies of any workspace project it links in. Selecting a project with `--filter` materializes the `link:` and `workspace:` targets it depends on *and* those targets' own dependencies, so the selected project runs without further installs.

pnpm 11 kept link targets shallow: `pnpm --filter project-1 install` linked `project-2` but left `project-2/node_modules` empty, and importing from it failed at runtime unless you knew to widen the selection with `pnpm --filter project-1... install`.

If you relied on the narrower behavior — a CI job that installs one project and wants nothing else on disk — the closure is now larger. `--filter <project>...` still works and is now redundant for this case.
