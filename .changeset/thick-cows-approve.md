---
"@pnpm/plugin-commands-audit": minor
"@pnpm/types": minor
"pnpm": minor
---

A new setting supported for ignoring vulnerabilities by their CVEs. The ignored CVEs may be listed in the `pnpm.auditConfig.ignoreCves` field of `package.json`. For instance:

```
{
  "pnpm": {
    "auditConfig": {
      "ignoreCves": [
        "CVE-2019-10742",
        "CVE-2020-28168",
        "CVE-2021-3749",
        "CVE-2020-7598"
      ]
    }
  }
}
```
