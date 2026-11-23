---
"pnpm": patch
---

When `pnpm-workspace.yaml` mostly uses double quotes, quoted values pnpm adds to it now use double quotes, such as the entries written by `pnpm config set`, `pnpm approve-builds`, and catalog updates. Previously only `minimumReleaseAgeExclude` and `trustPolicyExclude` entries followed the file's quote style [#18364](https://github.com/pnpm/pnpm/pull/18364).
