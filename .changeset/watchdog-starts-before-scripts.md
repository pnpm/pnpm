---
"pacquet": patch
---

Scripts that pnpm runs in a process group of their own are now ended when pnpm is killed right after starting them. A parallel run that was killed before pnpm registered its scripts left all of them running.
