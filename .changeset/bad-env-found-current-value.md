---
"@pnpm/os.env.path-extender-windows": patch
"@pnpm/os.env.path-extender": patch
"pnpm": patch
"pacquet": patch
---

On Windows, the `ERR_PNPM_BAD_ENV_FOUND` error of `pnpm setup` now shows the value `PNPM_HOME` is currently set to. It used to show the directory pnpm wanted to set instead.
