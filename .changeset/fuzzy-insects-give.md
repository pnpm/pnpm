---
"@pnpm/plugin-commands-installation": major
"@pnpm/plugin-commands-deploy": major
"@pnpm/store-connection-manager": major
"@pnpm/client": major
"pnpm": major
---

When there's a `files` field in the `package.json`, only deploy those files that are listed in it.
Use the same logic also when injecting packages. This behavior can be changed by setting the `deploy-all-files` setting to `true` [#5911](https://github.com/pnpm/pnpm/issues/5911).
