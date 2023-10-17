---
"@pnpm/plugin-commands-installation": minor
"@pnpm/plugin-commands-licenses": minor
"@pnpm/plugin-commands-patching": minor
"@pnpm/resolve-dependencies": minor
"@pnpm/package-is-installable": minor
"@pnpm/package-requester": minor
"@pnpm/store-controller-types": minor
"@pnpm/plugin-commands-store": minor
"@pnpm/license-scanner": minor
"@pnpm/filter-lockfile": minor
"@pnpm/headless": minor
"@pnpm/deps.graph-builder": minor
"@pnpm/core": minor
"@pnpm/types": minor
"@pnpm/config": minor
---

Support different architectures when installing dependencies.

Example:

```json
{
  "pnpm": {
    "supportedArchitectures": {
      "os": ["current", "win32"],
      "cpu": ["x64", "arm64"]
    }
  }
}
```
