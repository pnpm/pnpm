---
"pacquet": patch
---

A range whose upper bound leaves out a component, such as `<=16` or `<=2.0`, now matches every version that component stands for. `<=16` matched no 16.x version at all, so a peer dependency declared `>=0.11 <=3` was reported unmet by 3.0.1 [#14419](https://github.com/pnpm/pnpm/issues/14419).
