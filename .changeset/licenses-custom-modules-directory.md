---
"@pnpm/deps.compliance.license-scanner": patch
"pnpm": patch
---

`pnpm licenses list --json` now reports existing package paths when the isolated linker uses a custom `modulesDir`. The reported paths previously used the custom directory name inside virtual-store slots.
