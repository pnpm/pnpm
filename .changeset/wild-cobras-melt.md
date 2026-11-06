---
"pacquet": patch
---

Commands that add settings to `pnpm-workspace.yaml` now fail with an error if the file contains only `null`, and leave the file unchanged. Previously they wrote invalid YAML.
