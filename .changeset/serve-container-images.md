---
"@pnpm/pnpr": minor
---

pnpr now serves container images. Declare a registry with `ecosystem: oci`. Push to it with `docker`, `podman`, or `skopeo`. The distribution API answers at `/v2/` on the host root. An image keeps its own name, with no registry key in the path. Sign in with `docker login`, using a pnpr token as the password.
