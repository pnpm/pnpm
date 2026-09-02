---
"@pnpm/deps.github-actions": patch
"pnpm": patch
"pacquet": patch
---

`pnpm outdated` and `pnpm update` now follow local actions and reusable workflows referenced with GitHub's self-repository syntax (`uses: $/.github/actions/setup`) when looking for outdated GitHub Actions, the same way they follow `./` references.
