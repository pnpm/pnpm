---
"@pnpm/pkg-manifest.commands": minor
"pnpm": minor
---

Added `pnpm pkg get --published` to output the publish-transformed manifest without packing or publishing. The `--published` flag applies all publish-time transformations (`publishConfig` overrides, `workspace:` protocol resolution, catalog resolution, script stripping, etc.) and returns the result. Supports field selection (`pnpm pkg get exports --published`) and `--recursive`.
