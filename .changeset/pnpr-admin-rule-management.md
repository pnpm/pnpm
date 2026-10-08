---
"@pnpm/pnpr": minor
---

Admins can now change who may read, publish, and unpublish the packages of a hosted registry that sets `rulesManagedBy: api`. They use `/-/pnpr/v0/admin/rules/{registry}`, and the change applies without a config deploy. The package patterns a registry serves stay in the config [pnpm/tasks#111](https://github.com/pnpm/tasks/issues/111).
