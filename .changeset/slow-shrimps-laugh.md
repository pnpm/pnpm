---
"@pnpm/plugin-commands-script-runners": minor
"pnpm": minor
---

Added `--shell-mode`/`-c` option support to `pnpm exec` [#4328](https://github.com/pnpm/pnpm/pull/4328)

* `--shell-mode`: shell interpreter. See: https://github.com/sindresorhus/execa/tree/484f28de7c35da5150155e7a523cbb20de161a4f#shell

Usage example:

```shell
pnpm -r --shell-mode exec -- echo \"\$PNPM_PACKAGE_NAME\"
pnpm -r -c exec -- echo \"\$PNPM_PACKAGE_NAME\"
```

```json
{
    "scripts": {
        "check": " pnpm -r --shell-mode exec -- echo \"\\$PNPM_PACKAGE_NAME\""
    }
}
```
