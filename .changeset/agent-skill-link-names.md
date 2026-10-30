---
"pacquet": minor
---

pnpm now links each agent skill of a dependency under the skill's own directory name, such as `.claude/skills/auth`, as the Agent Skills specification requires. Links named `pnpm-<package>-<skill>` are replaced on the next install, and pnpm lists the linked skills in a `# pnpm` block of each agent skill directory's `.gitignore` [#17455](https://github.com/pnpm/pnpm/discussions/17455).
