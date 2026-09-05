---
"pacquet": patch
---

Python wheel downloads send credentials only over HTTPS or to a loopback address. Failed Python index requests no longer expose signed URLs in network errors.

Locked Python installs keep download slots busy when an earlier wheel is slow.

Authenticated metadata requests enforce configured redirect guards during manual redirects.
