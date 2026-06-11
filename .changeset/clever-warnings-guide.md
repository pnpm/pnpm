---
"@pnpm/config.reader": patch
"pnpm": patch
---

Improved the warning printed when a project `.npmrc` uses an environment variable in a registry/proxy URL or in registry credentials. The message now explains why the setting was ignored and how to migrate it to a trusted source — for example by moving the line to the user-level `~/.npmrc` or running `pnpm config set "<key>" <value>` — with a link to https://pnpm.io/npmrc. The `pnpm config set` example is only suggested when the key has no `${...}` placeholder, so the snippet is always safe to copy-paste.
