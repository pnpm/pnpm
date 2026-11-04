---
"pacquet": minor
---

pnpm now links an agent skill as `pnpm-<package>-<skill>` when two approved packages ship a skill of the same name, or when the skill's name is already taken in an agent skill directory. Before, the install failed.
