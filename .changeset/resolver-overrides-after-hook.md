---
"pacquet": patch
---

`overrides` are now applied after the `readPackage` hook during resolution, matching the TypeScript CLI's hook order (`packageExtensions` → `readPackage` hooks → `overrides`). A hook that replaced a manifest — such as a host application substituting a workspace project's raw manifest for its injected instances — previously erased the overrides from that manifest, so the resolved graph ignored them.
