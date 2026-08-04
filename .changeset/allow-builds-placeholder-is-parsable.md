---
"pacquet": patch
---

An `allowBuilds` entry with the `set this to true or false` placeholder pnpm scaffolds no longer makes every command in that workspace fail with a config-parse error [#13322](https://github.com/pnpm/pnpm/issues/13322). An undecided entry now leaves the package under the default-deny build policy, as pnpm does.
