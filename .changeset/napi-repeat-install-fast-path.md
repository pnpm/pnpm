---
"pacquet": patch
"@pnpm/napi": patch
---

Repeat installs through the Node-API bindings now return "Already up to date" when the project manifests still match `pnpm-lock.yaml`. Before, every such install reinstalled the whole tree. An install also no longer reinstalls when `pnpm-lock.yaml` differs from the installed dependencies only by packages no project depends on or by top-level keys pnpm does not define.
