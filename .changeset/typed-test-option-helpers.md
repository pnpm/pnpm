---
"@pnpm/testing.temp-store": patch
---

`createTempStore`'s `storeOptions` are typed as a partial, which is how they are used: they are spread over the store's own defaults, so a caller that overrides one knob no longer has to restate the other five.
