---
"@pnpm/pnpr": patch
---

A staged publish is now approved once, whichever replica of a shared registry the approval reaches. A second approval of the same stage answers 409. Rejecting a stage stops an approval that has not published it yet [#12199](https://github.com/pnpm/pnpm/issues/12199).
