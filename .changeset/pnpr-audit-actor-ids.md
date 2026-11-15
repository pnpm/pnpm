---
"@pnpm/pnpr": minor
---

pnpr audit records now carry `actor_id`, the account ID of whoever performed the action, which stays the same when the account is renamed. Sign-ins, failed logins, workload credential use, token creation and revocation, and publishes are now audited as `auth.sign_in`, `auth.login_failed`, `auth.workload_use`, `token.create`, `token.revoke`, and `package.publish`. Version and publication note events now name the actor by account name, and every event about another account names it with both `user` and `user_id`.
