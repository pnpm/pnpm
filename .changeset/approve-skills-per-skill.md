---
"pacquet": minor
---

`pnpm approve` now asks which agent skills to link when a package ships more than one. pnpm records the chosen skills as a list, such as `permissions.<pkg>.skills: [guide, migrate]`. pnpm does not offer a skill outside the list for approval again. The first time a project approves a skill, `pnpm approve` also asks which agent directories to link skills into. pnpm records the answer under `skills.dirs`.
