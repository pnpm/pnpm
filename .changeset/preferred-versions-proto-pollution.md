---
"@pnpm/lockfile.preferred-versions": patch
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Fixed a prototype-pollution hazard when seeding preferred versions: a dependency named `__proto__` in a manifest or in `pnpm-lock.yaml` could write through `Object.prototype` (or crash the install) while the preferred-versions map was being built. The maps are now null-prototype objects, so crafted package names land as plain keys.
