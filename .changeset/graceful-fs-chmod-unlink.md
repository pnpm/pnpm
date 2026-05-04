---
"@pnpm/fs.graceful-fs": minor
---

Add `chmod` and `unlink` (promisified) to the exported fs interface so callers can perform mode changes and removals through the same EMFILE-queueing layer as the other operations.
