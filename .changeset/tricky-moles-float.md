---
"@pnpm/config": major
"pnpm": major
---

`pnpm install -g pkg` will add the global command only to a predefined location. pnpm will not try to add a bin to the global Node.js or npm folder. To set the global bin directory, either set the `PNPM_HOME` env variable or the [`global-bin-dir`](https://pnpm.io/npmrc#global-bin-dir) setting.
