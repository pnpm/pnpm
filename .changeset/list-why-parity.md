---
"pacquet": minor
---

`pnpm list` and `pnpm why` are now feature complete and behaviorally identical to the TypeScript CLI. `pnpm list` gained `--only-projects`, `--find-by` (finders declared in `.pnpmfile.cjs`), search by version range (`pnpm ls "foo@^2"`), subtree deduplication with `[deduped]` markers, peer/skipped annotations, the package-count summary, `--long` manifest details, resolved tarball URLs and absolute paths in `--json`/`--parseable` output, and `--depth` support for globally installed packages. `pnpm why` gained `--json`, `--parseable`, `--long`, `--prod`/`--dev`/`--no-optional`, `--find-by`, workspace project names in the reverse tree, dependency-field annotations, `[circular]`/`[deduped]` markers, peer-variant hashes, and the `Found N versions` summary.
