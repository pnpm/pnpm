---
"@pnpm/pnpr": patch
---

npm clients now display the server's explanation when a registry request fails. Authentication errors distinguish invalid tokens from missing credentials. A `401` response includes a `WWW-Authenticate` challenge.
