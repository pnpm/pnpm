---
"@pnpm/plugin-commands-audit": minor
---

Add --ignore-vulnerabilities flag, which can be used to automate the ignoring of CVE's with no resolution

i.e
```shell
> pnpm audit --ignore-vulnerabilities
```
Provide a comma-delimited list of CVE's to ignore those specifically, even if they have a resolution.
```shell
> pnpm audit --ignore-vulnerabilities CVE-2021-1234,CVE-2021-5678
```