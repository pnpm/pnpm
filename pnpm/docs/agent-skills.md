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

pnpm only looks one level deep, at `skills/<name>/SKILL.md`. A package can consist of nothing but skills.

## Approving skills

A skill is instructions that an agent follows in every session, and it may include scripts the agent runs. So pnpm links nothing until you approve the package.

During install, pnpm looks for skills in the direct dependencies of every project. The packages whose skills await approval are listed after the install:

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

An approval covers every skill of the package, in every later version. A git or tarball dependency is approved by its exact source or, for git, by its repository, with the same keys as [`allowBuilds`](./settings/build.md#allowbuilds).

If a package resolves to several versions in a workspace, the highest version decides which skills are linked.

## Where skills are linked

By default, pnpm links into the agent skill directories that already exist in the workspace root, such as `.claude/skills` or `.cursor/skills`. When the agent running pnpm identifies itself through an environment variable, its directory is created if it is missing:

| Variable | Directory |
|---|---|
| `CLAUDECODE` | `.claude/skills` |
| `CURSOR_AGENT` | `.cursor/skills` |
| `GEMINI_CLI` | `.gemini/skills` |

To choose the directories yourself, set [`skills.dirs`](./settings/build.md#skillsdirs). An empty list turns agent skills off.

Each skill is linked as `pnpm-<package>-<skill>`. A scoped package name has its `/` replaced by `+`, so the `auth` skill of `@acme/kit` is linked as `pnpm-@acme+kit-auth`. pnpm adds a `pnpm-*` rule to a `.gitignore` in each directory, because the links point into `node_modules`.

pnpm removes a link once the package is removed, the approval is revoked, or the package no longer ships the skill. It never replaces or removes an entry it did not create, and fails if one is in the way.

If a skill is approved but there is no directory to link it into, the install fails.

Global installs, `pnpm dlx`, and `pnpm deploy` do not link skills.
