---
"@pnpm/installing.deps-installer": minor
"@pnpm/store.cafs": minor
"@pnpm/worker": minor
"pacquet": patch
"pnpm": minor
---

An install that had to re-hash store files to verify them now reports it. If that cost more than a second, it says how long — `The integrity of N files was checked in 2.5s.` — and if it was quick but covered more than a thousand files, it names the cause instead: their timestamps changed since the store recorded them, which a backup tool, an antivirus scan or a copied store can do.
