---
"pacquet": patch
---

With `storeIndexFormat: files`, concurrent store-index lookups during resolution and prefetching no longer wait for one another: the shared index handle is unlocked before the entry file is read, so on a network filesystem the round trips overlap instead of queuing.
