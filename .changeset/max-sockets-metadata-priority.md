---
"pacquet": patch
---

Package metadata requests no longer wait behind queued tarball downloads when `maxSockets` or a proxy limits the connections to a registry. Large installs resolve faster and print fewer `Request took` warnings.
