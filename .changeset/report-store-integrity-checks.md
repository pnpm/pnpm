---
"@pnpm/installing.deps-installer": minor
"@pnpm/store.cafs": minor
"@pnpm/worker": minor
"pacquet": patch
"pnpm": minor
---

An install that spent more than a second re-hashing files to verify the store now says so, so a slow install has a visible cause: `The integrity of N files was checked in 2.5s. This might have caused installation to take longer.` The figures cover that install alone, so one project of a recursive workspace command no longer reports another's work.
