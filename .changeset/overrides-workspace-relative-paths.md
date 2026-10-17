---
"@pnpm/hooks.read-package-hook": patch
"pnpm": patch
"pacquet": patch
---

pnpm now measures a `pnpm.overrides` entry written as a `workspace:` path, such as `workspace:./packages/foo`, from the directory holding `pnpm-workspace.yaml`. It used to be measured from each package the override rewrote, so the install linked a directory that does not exist or failed when workspace packages were injected [#17010](https://github.com/pnpm/pnpm/issues/17010).
