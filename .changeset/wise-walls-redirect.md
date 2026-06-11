---
"@pnpm/config": patch
"pnpm": patch
---

A repository-controlled project or workspace `.npmrc` can no longer redirect which files pnpm loads as its trusted user and global configuration. Previously such a file could set `userconfig`, `globalconfig`, or `prefix` to point at an attacker-supplied file shipped in the repository, and pnpm would load it as a trusted config source — bypassing the protection that prevents repository config from expanding environment variables into registry request destinations and credentials, and allowing it to set `tokenHelper`. The user/global config file locations are now resolved only from trusted sources (CLI options, environment config, the npm builtin config, and defaults) before the project and workspace `.npmrc` files are read. Fixed by upgrading `@pnpm/npm-conf` to `3.0.3`.
