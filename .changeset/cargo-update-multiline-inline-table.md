---
"pacquet": patch
---

`pnpm update` now updates a Cargo dependency whose inline table spans several lines, such as one with a multi-line `features` array. It failed with "its Cargo.toml dependency declaration is not a string or single-line inline table".
