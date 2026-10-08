---
"pacquet": minor
---

`pnpm install` now links the agent skills that direct dependencies ship under `skills/<name>/SKILL.md` into the project's agent skill directories, such as `.claude/skills`. A package's skills are linked only after you approve them with `pnpm approve`. The `skills.dirs` setting chooses the directories [pnpm/rfcs#35](https://github.com/pnpm/rfcs/pull/35).
