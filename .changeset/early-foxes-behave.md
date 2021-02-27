---
"@pnpm/config": minor
---

New setting added: `modules-cache-max-age`. The default value of the setting is 10080 (7 days in seconds). `modules-cache-max-age` is the time in minutes after which pnpm should remove the orphan packages from node_modules.
