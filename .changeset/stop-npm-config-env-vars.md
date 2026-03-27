---
"@pnpm/exec.lifecycle": minor
"pnpm": minor
---

Stop setting `npm_config_*` environment variables from pnpm config during lifecycle scripts. This fixes `npm warn Unknown env config` warnings when lifecycle scripts invoke npm internally. Only well-known `npm_*` env vars (like `npm_lifecycle_event`, `npm_config_node_gyp`, `npm_config_user_agent`, etc.) are now set, matching Yarn's behavior.
