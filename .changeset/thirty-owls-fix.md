---
"@pnpm/plugin-commands-script-runners": patch
"pnpm": patch
---

The `pnpx`, `pnpm dlx`, `pnpm create`, and `pnpm exec` commands should set the `npm_config_user_agent` env variable [#3985](https://github.com/pnpm/pnpm/issues/3985).
