---
"@pnpm/pnpr": minor
---

pnpr now serves SCIM 2.0 user endpoints when `auth.scim.token` is set, so an identity provider can deprovision accounts. Deactivating or deleting a user revokes its tokens, removes its password account, ends its browser sessions, and refuses any later sign-in with that username.
