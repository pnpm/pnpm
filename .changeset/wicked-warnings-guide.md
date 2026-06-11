---
"@pnpm/config": patch
"pnpm": patch
---

Improved the warning printed when a project `.npmrc` uses an environment variable in a registry/proxy URL or in registry credentials. The message now explains why the setting was ignored and how to migrate it to a trusted source — for example by running `pnpm config set "<key>" <value>` to store it in the global config, or by keeping the `${...}` line in the user-level `~/.npmrc` — with a link to https://pnpm.io/npmrc.
