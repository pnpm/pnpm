---
"@pnpm/pnpr": patch
---

Password changes now revoke existing bearer tokens. Users can change their password with `npm profile set password`.

Failed login attempts now share a per-account limit across client IPs. A full registration limit returns the same authentication error for unknown accounts and incorrect passwords.

Authentication errors for team administration, password verification, and logout now identify the requested resource or username correctly.
