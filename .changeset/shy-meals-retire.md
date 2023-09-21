---
"@pnpm/reviewing.dependencies-hierarchy": patch
---

Fix memory error when running `bit why` in large trees. 
The function will now limit the end leafs to 10, which also makes the output more readable.
