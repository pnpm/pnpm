---
"pacquet": minor
---

pnpm now names the link to a dependency's agent skill after the skill alone. A skill that the `@acme/kit` package ships in `skills/auth/` is linked as `.claude/skills/auth`, not `.claude/skills/pnpm-@acme+kit-auth`. The next install replaces links with the old names. pnpm lists the linked skills in a `# pnpm` block of each agent skill directory's `.gitignore` [#17455](https://github.com/pnpm/pnpm/discussions/17455).
