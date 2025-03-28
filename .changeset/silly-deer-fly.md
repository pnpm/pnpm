---
"@pnpm/plugin-commands-installation": major
"@pnpm/pnpmfile": minor
"@pnpm/cli-utils": minor
"pnpm": minor
---

**Experimental.** A new hook is supported for updating configuration settings. The hook can be provided via `.pnpmfile.cjs`. For example:

```js
module.exports = {
  hooks: {
    updateConfig: (config) => ({
      ...config,
      nodeLinker: 'hoisted',
    }),
  },
}
```

