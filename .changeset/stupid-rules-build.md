---
"@pnpm/plugin-commands-audit": minor
"pnpm": minor
---

Added two new flags to the `pnpm audit` command, `--ignore` and `--ignore-unfixable` [#8474](https://github.com/pnpm/pnpm/pull/8474).

Ignore all vulnerabilities that have no solution:

```shell
> pnpm audit --ignore-unfixable
```

Provide a list of CVE's to ignore those specifically, even if they have a resolution.

```shell
> pnpm audit --ignore=CVE-2021-1234 --ignore=CVE-2021-5678
```
