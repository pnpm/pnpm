---
"pacquet": patch
---

pnpm now keeps archive extraction within its concurrency limit when an install abandons a download whose extraction is still running. The extraction slot was released as soon as the install stopped waiting for it, letting the next extraction start too early [#14585](https://github.com/pnpm/pnpm/issues/14585).
