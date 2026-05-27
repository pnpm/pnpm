---
"@pnpm/fetching.git-fetcher": patch
"pnpm": patch
---

Reject git resolutions whose `commit` field is not a 40-character hexadecimal SHA before invoking `git`. A malicious lockfile could otherwise smuggle a value such as `--upload-pack=<command>` through `git fetch` / `git checkout`, which on SSH or local-file transports executes the supplied command.
