---
"@pnpm/plugin-commands-installation": minor
"@pnpm/workspace.state": minor
"@pnpm/types": minor
"@pnpm/deps.status": minor
"pnpm": minor
---

Added support for a new type of dependencies called "configurational dependencies". These dependencies are installed before all the other types of dependencies (before "dependencies", "devDependencies", "optionalDependencies").

Configurational dependencies cannot have dependencies of their own or lifecycle scripts. They should be added using exact version and the integrity checksum. Example:

```json
{
  "pnpm": {
    "configDependencies": {
      "my-configs": "1.0.0+sha512-30iZtAPgz+LTIYoeivqYo853f02jBYSd5uGnGpkFV0M3xOt9aN73erkgYAmZU43x4VfqcnLxW9Kpg3R5LC4YYw=="
    }
  }
}
```

Related RFC: [#8](https://github.com/pnpm/rfcs/pull/8).
Related PR: [#8915](https://github.com/pnpm/pnpm/pull/8915).
