---
"@pnpm/deps.peer-range": patch
"pnpm": patch
"pacquet": patch
---

`pnpm peers check` no longer reports a peer dependency declared as `workspace:^`, `workspace:~`, or a bare `workspace:` as unmet. pnpm reported these as unmet whatever version the linked workspace project supplied [#14770](https://github.com/pnpm/pnpm/issues/14770).
