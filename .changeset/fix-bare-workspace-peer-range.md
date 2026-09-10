---
"@pnpm/deps.peer-range": patch
"pnpm": patch
"pacquet": patch
---

Fixed a bare `workspace:` shorthand peer dependency (`workspace:^`, `workspace:~`, or `workspace:` with no version) always being reported as an unmet peer by `pnpm peers check`, regardless of what was actually installed. See pnpm/pnpm#14770.
