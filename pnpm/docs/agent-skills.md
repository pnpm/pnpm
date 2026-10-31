---
id: agent-skills
title: Agent Skills
---

Added in: v12.11.0

A package can ship agent skills: directories with a `SKILL.md` that AI coding agents load as instructions. pnpm links the skills of approved dependencies into the agent skill directories of your project, so the agent working in the repository sees them.

## Shipping skills in a package

Put each skill in its own directory under `skills/`:

```
my-package/
├── package.json
└── skills/
    └── migrations/
        ├── SKILL.md
        └── reference.md
```

pnpm looks for skills up to three levels below `skills/`: `skills/<name>/SKILL.md`, and category paths such as `skills/engineering/<name>/SKILL.md`. Name each skill directory after the `name` in its `SKILL.md`, as the [Agent Skills specification](https://agentskills.io/specification) requires. A package can consist of nothing but skills.

## Approving skills

A skill is instructions that an agent follows in every session, and it may include scripts the agent runs. So pnpm links nothing until you approve the package.

During install, pnpm looks for skills in the direct dependencies of every project. Only the dependency groups the install includes count, so `pnpm install --prod` skips the skills of `devDependencies`. The packages whose skills await approval are listed after the install:

```
Agent skills awaiting approval: drizzle-kit.
Run "pnpm approve" to review them.
```

A pending skill never fails the install. Run [`pnpm approve`](./cli/permissions.md) to decide. The decision is recorded in the [`permissions`](./settings/build.md#permissions) setting:

```yaml title="pnpm-workspace.yaml"
permissions:
  drizzle-kit:
    skills: true
```

When a package ships several skills, the approval prompt lists them and you choose which ones to link:

```yaml title="pnpm-workspace.yaml"
permissions:
  drizzle-kit:
    skills: [guide, migrate]
```

Only the skills in the list are linked, and a skill you leave out is never offered again. `skills: true` links every skill the package ships, in every later version. A git or tarball dependency is approved by its exact source or, for git, by its repository, with the same keys as [`allowBuilds`](./settings/build.md#allowbuilds).

If a package resolves to several versions in a workspace, the highest version decides which skills are linked. If the highest version ships no skills, none are linked, even when an older version does. If one version is installed several times with different peer dependencies, every install links the skills of the same copy, as long as the set of copies does not change. pnpm treats a link to the same skill of the same package in the virtual store as its own, even if that copy is no longer in the lockfile.

## Where skills are linked

By default, pnpm links into the agent skill directories that already exist in the workspace root: every `.<name>/skills` directory, such as `.claude/skills`, `.cursor/skills`, or `.agents/skills`. When the agent running pnpm identifies itself through an environment variable, its directory is created if it is missing:

| Variable | Directory |
|---|---|
| `CLAUDECODE`, `CLAUDE_CODE` | `.claude/skills` |
| `CURSOR_AGENT` | `.cursor/skills` |
| `GEMINI_CLI` | `.gemini/skills` |
| `ANTIGRAVITY_AGENT` | `.agents/skills` |
| `COPILOT_AGENT`, `COPILOT_CLI` | `.github/skills` |
| `CODEX_THREAD_ID`, `CODEX_SANDBOX` | `.agents/skills` |
| `AI_AGENT` | `.agents/skills` |

To choose the directories yourself, set [`skills.dirs`](./settings/build.md#skillsdirs). An empty list turns agent skills off. The first time a project approves a skill, `pnpm approve` lists the detected directories and asks which ones to link into, so you rarely write this setting by hand.

Each skill is linked under the name of its directory, so the `auth` skill of `@acme/kit` is linked as `auth`. When the name is taken somewhere, or two approved packages ship a skill of the same name, pnpm links it as `pnpm-<package>-<skill>` instead, in every agent directory. A scoped package name has its `/` replaced by `+`, so the `auth` skill of `@acme/kit` is linked as `pnpm-@acme+kit-auth`. A tarball or local directory dependency is linked under the alias the project gives it, not its package name. Because the links point into `node_modules`, pnpm lists them in a `# pnpm` block of a `.gitignore` in each directory:

```gitignore title=".claude/skills/.gitignore"
# pnpm
/auth
# pnpm end
```

pnpm rewrites the block on every install and leaves the other lines of the file alone.

pnpm removes a link once the package is removed, the approval is revoked, or the package no longer ships the skill. An entry pnpm did not create makes the skill fall back to the prefixed name, and if even that name is taken by one, the install fails.

If a skill is approved but there is no directory to link it into, the install fails.

Global installs, `pnpm dlx`, and `pnpm deploy` do not link skills.
