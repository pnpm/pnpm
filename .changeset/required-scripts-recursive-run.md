---
"pacquet": patch
---

`pnpm -r run <script>` now honors the `requiredScripts` setting from `pnpm-workspace.yaml`. If the script is listed there and a selected project does not define it, the run fails with `ERR_PNPM_RECURSIVE_RUN_NO_SCRIPT` and names the projects that lack it.
