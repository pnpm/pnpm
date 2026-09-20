---
"pacquet": patch
---

`pnpm pipeline --watch` now names the agent's checkout after the repository when `--repo` is given a Windows path. The Cargo build cache resolves the project and repository paths before it keys an entry, so two spellings of one directory, such as a Windows 8.3 short path, share the cached build state [#15105](https://github.com/pnpm/pnpm/issues/15105).
