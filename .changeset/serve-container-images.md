---
"@pnpm/pnpr": minor
---

pnpr now serves container images. Declare a registry with `ecosystem: oci` and push to it with `docker`, `podman`, or `skopeo`. The distribution API answers at `/v2/` on the host root, and an image is addressed by its own name with no registry key in the path. Sign in with `docker login` using a pnpr token as the password. Proxying an upstream image registry is not supported yet.
