---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
---

Fixed `strictDepBuilds` and `allowBuilds` checks being bypassed when a package's build approval is revoked after side-effects were cached in the store. Now detects packages that were previously in `allowBuilds: true` but are no longer approved, and adds them to `ignoredBuilds` so `strictDepBuilds` fails and the warning is shown.
