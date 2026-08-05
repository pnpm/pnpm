---
"@pnpm/napi": patch
---

Fixed `resolveDependency({ fullMetadata: true })` returning a manifest stripped down to the abbreviated npm field set. Registry-custom fields on the version object (such as Bit's `componentId`) are now preserved.
