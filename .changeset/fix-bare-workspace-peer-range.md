---
"@pnpm/deps.peer-range": patch
"pnpm": patch
"pacquet": patch
---

`pnpm peers check` no longer reports a peer dependency declared as `workspace:^`, `workspace:~`, or a bare `workspace:` as unmet. These shorthands carry no version, so pnpm compared the installed version against an unusable range that nothing could satisfy [#14770](https://github.com/pnpm/pnpm/issues/14770).
