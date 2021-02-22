---
"@pnpm/tarball-resolver": patch
---

The ID of a tarball dependency should not contain colons, when the URL has a port. The colon should be escaped with a plus sign.
