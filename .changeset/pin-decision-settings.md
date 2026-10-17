---
"pacquet": patch
---

pnpm now reads only the settings it needs to decide whether to switch to the pnpm version a project pins. A setting outside them can no longer fail that decision or change its outcome. For example, `pnpm --version` no longer fails in a pinned project whose `pnpm-workspace.yaml` has a setting this pnpm cannot read. Projects that pin neither pnpm nor a runtime skip this check without reading any settings.
