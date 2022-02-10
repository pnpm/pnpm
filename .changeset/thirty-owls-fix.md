---
"@pnpm/plugin-commands-script-runners": patch
"pnpm": patch
---

The `dlx`, `create`, and `exec` commands should set the `npm_config_user_agent` env variable.
