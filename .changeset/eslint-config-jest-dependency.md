---
"@pnpm/eslint-config": patch
---

Declared `eslint-plugin-jest` as a dependency. The config imports it, so it failed to load in projects that did not install the plugin themselves.
