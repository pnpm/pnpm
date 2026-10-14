---
"@pnpm/pnpr": patch
---

Fixed several security and consistency issues in pnpr. Auth files default to 0600 permissions, login requests on disabled registration return 401 to prevent user enumeration, passwords are validated against bcrypt truncation, and published package versions remain immutable across partial unpublish requests.
