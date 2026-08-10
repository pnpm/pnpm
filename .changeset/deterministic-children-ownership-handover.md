---
"pacquet": patch
---

Fixed dependency resolution producing a different lockfile depending on the order in which the occurrences of a shared package finished resolving. When an occurrence of a package took over its children and resolved different child versions than the occurrence that recorded them first, the other occurrences kept pointing at the old versions, so the resolved graph depended on which occurrence ran first.
