---
"@pnpm/plugin-commands-publishing": patch
"pnpm": patch
---

Replaced unsafe `publish as unknown as OtpPublishFn` type cast with a stricter intermediate type that explicitly bridges only the manifest parameter difference between `@types/libnpmpublish`'s outdated `PackageJson` and `ExportedManifest`.
