---
"pacquet": patch
---

When pnpm revalidates cached registry metadata, it now also sends `If-Modified-Since` if the registry's modification time is a date without a time.
