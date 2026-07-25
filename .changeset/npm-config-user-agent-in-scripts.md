---
"pacquet": patch
---

`npm_config_user_agent` now carries the configured user agent (`pnpm/<version> …`) in install lifecycle scripts, `pnpm run`, `pnpm exec`, and `pnpm dlx` [#13322](https://github.com/pnpm/pnpm/issues/13322). It was previously unset for install scripts and the bare string `pnpm` elsewhere, which made `preinstall` guards that check for pnpm reject the install.
