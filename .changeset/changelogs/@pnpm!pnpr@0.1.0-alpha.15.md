## 0.1.0-alpha.15

### Minor Changes

- Admins can now change who may read, publish, and unpublish the packages of a hosted registry that sets `rulesManagedBy: api`. They use `/-/pnpr/v0/admin/rules/{ecosystem}/{name}`, and the change applies without a config deploy. The package patterns a registry serves stay in the config [pnpm/tasks#111](https://github.com/pnpm/tasks/issues/111).

- Admins can now manage teams with `pnpm team` on a hosted registry that sets `teamsManagedBy: api`. List the admins in `auth.admins`. pnpr stores these teams in the hosted store, so every replica serves the same teams, and a `team:` access rule applies a membership change immediately [pnpm/tasks#111](https://github.com/pnpm/tasks/issues/111).

  Browser origins listed in `cors.allowedOrigins` may now send `PUT`, `POST`, and `DELETE` requests.

- Admins listed in `auth.admins` can now manage accounts through `/-/pnpr/v0/admin/users`. They can create accounts while self-registration is disabled, change passwords, remove accounts, and revoke tokens. Removing an account also revokes its tokens [pnpm/tasks#111](https://github.com/pnpm/tasks/issues/111).

- `npm access grant`, `npm access revoke`, and `npm access list packages <scope:team>` now work on a registry with `rulesManagedBy: api`. A grant adds the team to the `access` list of a package the `packages:` map declares by name, and `read-write` also adds it to `publish`.

- OIDC sign-in can now put a user in registry teams based on the groups in their ID token. Configure it under `auth.oidc[].login.groups`. The membership lasts as long as the session and does not apply to the user's password logins or tokens [pnpm/tasks#111](https://github.com/pnpm/tasks/issues/111).

- The package rules admin API now reports a `version` and an `ETag`, and accepts `If-Match` on `PUT` and `DELETE`. A change based on rules another admin has since changed is refused with `412`.

- pnpr now serves SCIM 2.0 user endpoints when `auth.scim.token` is set, so an identity provider can deprovision accounts. Deactivating or deleting a user revokes its tokens, removes its password account, ends its browser sessions, and refuses any later sign-in with that username.

- The web UI can now sign in with a password or with an OIDC provider that offers browser sign-in. pnpr lists those providers at `GET /-/pnpr/v0/sign-in`. A browser sign-in started with `/-/oidc/<provider>/login?return=ui` ends in the web UI.

- pnpr serves a web UI at `/-/ui/` when the new `@pnpm/pnpr-ui` package is installed next to it, for example with `pnpm add -g @pnpm/pnpr,@pnpm/pnpr-ui`. The new `ui` setting turns the UI off or serves it from another directory.

### Patch Changes

- pnpr now computes a missing or empty `dist.integrity` for upstream package versions, so pnpm clients can install them. It pins up to 64 versions each time it fetches a packument from an upstream with caching enabled [pnpm/tasks#24](https://github.com/pnpm/tasks/issues/24).

- pnpr can now cache images from Quay. Layer downloads that Quay redirects to its CDN no longer fail with a 502 [#16548](https://github.com/pnpm/pnpm/issues/16548).

- A token now stops working as soon as its account is removed through pnpr. Before, it kept working until it was revoked. Container registry tokens issued from it stop too. An account removed from the htpasswd file by hand stops its tokens at the next restart.

  A new account with the name of a removed one no longer accepts tokens the removed account left behind.
