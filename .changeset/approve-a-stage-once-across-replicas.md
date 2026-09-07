---
"@pnpm/pnpr": patch
---

A staged publish is now approved once, whichever replica of a shared hosted store the approval reaches. The approving request claims the record with a conditional write, so a second approval of the same stage answers 409 instead of replaying the held publish [#12199](https://github.com/pnpm/pnpm/issues/12199).
