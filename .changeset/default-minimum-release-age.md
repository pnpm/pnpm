---
"@pnpm/config.reader": major
"pnpm": major
---

The default value of the `minimumReleaseAge` setting is now 1440 minutes (1 day). Newly published packages will not be resolved until they are at least 1 day old. This protects against supply chain attacks by giving the community time to detect and remove compromised versions. To opt out, set `minimumReleaseAge: 0` in `pnpm-workspace.yaml`.
