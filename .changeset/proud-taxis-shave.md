---
"pacquet": patch
---

A forced full re-resolution (config changes the fast lockfile update cannot absorb, such as a changed override or `packageExtensions`) no longer moves dependencies whose recorded versions still satisfy their ranges. The prior lockfile now pins each still-satisfied edge even when its recorded subtree cannot be reused wholesale, so open ranges like `@types/node: "*"` keep their locked versions instead of collapsing onto the highest locked version and churning the lockfile.
