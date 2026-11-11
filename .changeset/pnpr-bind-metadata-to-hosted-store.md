---
"@pnpm/pnpr": patch
---

`pnpr` now writes a random `store_id` into `layout.json` when initializing a hosted store. The `store_id` is also recorded in the metadata database. `pnpr` refuses to start, collect (`pnpr gc`), verify (`pnpr verify`), export (`pnpr export`), or migrate (`pnpr migrate-storage`) when the store IDs in `layout.json` and the metadata database differ or when only one side holds one [zkochan/pnpm-tasks#62](https://github.com/zkochan/pnpm-tasks/issues/62).
