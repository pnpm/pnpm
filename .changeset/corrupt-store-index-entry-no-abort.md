---
"pnpm": patch
---

pnpm no longer aborts on a corrupt entry in the store index. Reading an entry that claims more data than it holds, or that nests more than 1024 levels deep, now fails like any other unreadable entry.
