---
"pacquet": patch
---

Fixed a lockfile corruption during non-frozen re-installs: when one workspace project reused a package's resolution from the lockfile and another project's edge to the same package was denied reuse (for example because it also depends on a direct dependency whose specifier changed), the denied edge could read the reused, dependency-less resolution from the shared wanted-dependency cache and record the package as a leaf. Its lockfile snapshot became empty (`{}`), its peer suffix was dropped, and none of its dependencies were linked, which later broke installs and builds consuming that lockfile [#13070](https://github.com/pnpm/pnpm/pull/13070).
