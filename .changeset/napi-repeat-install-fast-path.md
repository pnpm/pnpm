---
"pacquet": patch
"@pnpm/napi": patch
---

An `install` through the Node-API bindings now returns "Already up to date" without running the install when nothing changed since the previous install. The bindings receive the project manifests in memory, so the check compares every manifest with `pnpm-lock.yaml` by content instead of by the `package.json` modification time. A repeat install that reaches the lockfile check no longer re-installs when the wanted lockfile differs from the installed one only by a package no project depends on or by a top-level key pnpm does not define.
