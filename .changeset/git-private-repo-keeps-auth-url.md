---
"pacquet": patch
---

A private git-hosted dependency resolved over HTTPS with an embedded auth token (`git+https://<token>@github.com/owner/repo.git`) is now recorded as a `type: git` resolution against the authenticated remote, instead of being rewritten to the host's public archive URL (a `codeload.github.com` tarball) that carries none of those credentials and so could not be fetched.
