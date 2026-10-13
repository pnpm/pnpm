---
"pacquet": patch
---

With `storeIndexFormat: files`, a store-index lookup held the shared index lock while it read the entry file, so concurrent lookups during resolution and prefetching waited for one another. The lock is now released before the file is read, and on a network filesystem the round trips overlap.
