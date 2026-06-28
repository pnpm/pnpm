---
"@pnpm/pkg-manifest.commands": minor
"pnpm": minor
---

Added `pnpm pkg get-published` subcommand to output the publish-transformed manifest without packing or publishing. It applies all publish-time transformations (`publishConfig` overrides, `workspace:` protocol resolution, catalog resolution, script stripping, etc.) and returns the result. Supports field selection (`pnpm pkg get-published exports`) and `--recursive`.

pacquet port not needed — `pkg` is outside pacquet's current surface area (dependency-management commands only).
