---
"@pnpm/package-requester": minor
---

In some cases, a filtered install (i.e. `pnpm install --filter ...`) was slower than running `pnpm install` without any filter arguments. This performance regression is now fixed. Filtered installs should be as fast or faster than a full install.
