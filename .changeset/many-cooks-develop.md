---
"@pnpm/get-context": major
---

`PnpmContext.hoistPattern` and `PnpmContext.publicHoistPattern` are no longer affected by modules directory state [#8657](https://github.com/pnpm/pnpm/pull/8657). Prior behavior can be recreated with the new properties `PnpmContext.currentHoistPattern` (`_.currentHoistPattern ?? _.hoistPattern`) and `PnpmContext.currentPublicHoistPattern` (`_.currentPublicHoistPattern ?? _.publicHoistPattern`).
