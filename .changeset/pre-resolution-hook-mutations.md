---
"pacquet": patch
---

Changes a `preResolution` hook makes to the lockfiles now take effect. The hook now also runs when pnpm installs from an up-to-date lockfile. An error thrown by the hook now fails the install.
