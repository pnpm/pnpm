---
"pacquet": patch
---

`pnpm install --ignore-pnpmfile` is accepted again. The flag skips every pnpmfile hook for the install: neither the workspace `.pnpmfile.cjs` nor the pnpmfiles of config-dependency plugins are loaded.
