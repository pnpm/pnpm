---
"pacquet": minor
---

`pnpm approve` now asks which of a package's agent skills to link when the package ships more than one. The chosen ones are recorded as a list, such as `permissions.<pkg>.skills: [guide, migrate]`, and a skill outside the list is not offered for approval again. The first time a project approves a skill, `pnpm approve` also asks which agent directories the skills are linked into, recording the answer under `skills.dirs`.
