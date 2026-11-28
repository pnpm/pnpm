---
"pnpm": patch
---

pnpm no longer aborts on a corrupt entry in the store index. An entry whose damage makes it claim more data than it holds now fails like any other unreadable entry.
