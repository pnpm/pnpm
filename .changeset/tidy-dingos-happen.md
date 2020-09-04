---
"@pnpm/default-reporter": minor
---

Sometimes, when installing new dependencies that rely on many peer dependencies, or when running installation on a huge monorepo, there will be hundreds or thousands of warnings. Printing many messages to the terminal is expensive and reduces speed, so pnpm will only print a few warnings and report the total number of the unprinted warnings.
