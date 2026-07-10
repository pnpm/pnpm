---
"@pnpm/hooks.types": minor
"@pnpm/fetching.pick-fetcher": minor
"pnpm": minor
---

Custom fetchers exported from a pnpmfile can now delegate by returning a `{ delegate: <resolution> }` envelope: pnpm rewrites the package's resolution to the delegated shape and runs the built-in fetcher on it. This is the portable delegation form that also works in pacquet, where `cafs` and `fetchers` cannot be passed to the hook. Related to [pnpm/pnpm#11685](https://github.com/pnpm/pnpm/issues/11685).
