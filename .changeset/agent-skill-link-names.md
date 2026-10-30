---
"pacquet": minor
---

pnpm now links each agent skill of a dependency under the skill's own directory name, such as `.claude/skills/auth`. pnpm replaces links named `pnpm-<package>-<skill>` on the next install. pnpm lists the linked skills in a `# pnpm` block of each agent skill directory's `.gitignore` [#17455](https://github.com/pnpm/pnpm/discussions/17455).
