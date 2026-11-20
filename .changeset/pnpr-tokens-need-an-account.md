---
"@pnpm/pnpr": patch
---

A token now stops working as soon as its account is removed through pnpr. Before, it kept working until it was revoked. Container registry tokens issued from it stop too. An account removed from the htpasswd file by hand stops its tokens at the next restart.

A new account with the name of a removed one no longer accepts tokens the removed account left behind.
