---
"pacquet": minor
---

An install that resolves the dependency graph now reports the unmet peer dependencies it leaves behind, matching the TypeScript CLI. By default it warns once — `Issues with peer dependencies found. Run "pnpm peers check" to list them.` — and with `strictPeerDependencies` it fails with `ERR_PNPM_PEER_DEP_ISSUES` after the artifacts are written, listing every unmet peer. This covers `pnpm install`, `add`, `remove`, `update` and `--lockfile-only`; `pnpm dedupe` reported the same verdict already, and now shares the reporting with them. `peerDependencyRules` are applied before the verdict, so a rule that covers every issue leaves nothing to report, and a `--filter`ed install reports only on the projects it installed. An install that skips resolution — a frozen install, or one whose `pnpm-lock.yaml` is already up to date — reports nothing, as in the TypeScript CLI; `pnpm peers check` inspects such a tree [#14098](https://github.com/pnpm/pnpm/issues/14098).
