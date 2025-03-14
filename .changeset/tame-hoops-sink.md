---
"@pnpm/worker": minor
"pnpm": minor
---

The max amount of pnpm workers running during installation has been reduced to 4 to achieve optimal results [#9286](https://github.com/pnpm/pnpm/issues/9286). The workers are performing many file system operations, so increasing the number of CPUs doesn't help performance after some point.
