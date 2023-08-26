---
"@pnpm/config": major
"pnpm": minor
---

The default value of the `resolution-mode` setting is changed to `highest`. This setting was changed to `lowest-direct` in v8.0.0 and some users were [not happy with the change](https://github.com/pnpm/pnpm/issues/6463). A [twitter poll](https://twitter.com/pnpmjs/status/1693707270897517022) concluded that most of the users want the old behaviour (`resolution-mode` set to `highest` by default). This is a semi-breaking change but should not affect users that commit their lockfile [#6463](https://github.com/pnpm/pnpm/issues/6463).
