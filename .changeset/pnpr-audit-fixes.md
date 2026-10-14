---
"@pnpm/pnpr": patch
---

New auth files default to 0600 permissions on Unix. Login requests on disabled registration return 401 to prevent user enumeration. Passwords exceeding 71 bytes are rejected to prevent bcrypt truncation. Published package versions remain immutable across partial unpublish requests.
