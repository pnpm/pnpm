---
"@pnpm/parse-cli-args": patch
"pnpm": patch
---

Allow scope registry CLI option without `--config.` prefix such as `--@scope:registry=https://scope.example.com/npm` [#9089](https://github.com/pnpm/pnpm/pull/9089).
