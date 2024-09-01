---
"@pnpm/plugin-commands-audit": minor
"@pnpm/audit": minor
"@pnpm/types": minor
"pnpm": minor
---

Added a new setting to `package.json` at `pnpm.auditConfig.ignoreGhsas` for ignoring vulnerabilities by their GHSA code [#6838](https://github.com/pnpm/pnpm/issues/6838).

For instance:

```json
{
  "pnpm": {
    "auditConfig": {
      "ignoreGhsas": [
        "GHSA-42xw-2xvc-qx8m",
        "GHSA-4w2v-q235-vp99",
        "GHSA-cph5-m8f7-6c5x",
        "GHSA-vh95-rmgr-6w4m"
      ]
    }
  }
}
```
