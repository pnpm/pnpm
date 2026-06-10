---
"@pnpm/config.package-is-installable": patch
"pnpm": patch
---

Platform-specific optional dependencies are now skipped even when their `os`/`cpu`/`libc` fields are missing from the registry metadata or the lockfile. Some registries strip these fields from the package metadata, which made pnpm download and install the binaries of every platform regardless of `supportedArchitectures`. The missing platform fields of an optional dependency are now inferred from its name (e.g. `@nx/nx-win32-arm64-msvc` → `os: win32`, `cpu: arm64`), so foreign-platform binaries are skipped without even downloading them [#11702](https://github.com/pnpm/pnpm/issues/11702).
