---
"@pnpm/render-peer-issues": minor
"pnpm": patch
---

When reporting unmet peer dependency issues, if the peer dependency is resolved not from a dependency installed by the user, then print the name of the parent package that has the bad peer dependency installed as a dependency.

![](https://i.imgur.com/0kjij22.png)
