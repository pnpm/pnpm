---
"pacquet": patch
---

Improved install performance: the store-index writer's shutdown now overlaps the install's final lockfile and `.modules.yaml` writes instead of extending the install's tail.
