---
"pacquet": patch
---

Fixed a slowdown at the end of a resolving install in a large workspace. The peer-dependency report now inspects only the projects the resolution flagged, rather than every project in the lockfile ([pnpm/pnpm#14359](https://github.com/pnpm/pnpm/issues/14359)).
