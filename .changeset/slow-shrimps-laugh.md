---
"@pnpm/plugin-commands-script-runners": patch
---

Added --shell-mode option support to pnpm exec [#4328](https://github.com/pnpm/pnpm/pull/4328)

* `--shell-mode`: shell interpreter. see: https://github.com/sindresorhus/execa/tree/484f28de7c35da5150155e7a523cbb20de161a4f#shell

Usage example:

```shell
pnpm --recursive --shell-mode exec -- echo \"\$PNPM_PACKAGE_NAME\"
```

```json
{
    "scripts": {
        "check": " pnpm --recursive --shell-mode exec -- echo \"\\$PNPM_PACKAGE_NAME\""
    }
}
```
