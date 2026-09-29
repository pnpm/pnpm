---
id: git-worktrees
title: pnpm + Git Worktrees for Multi-Agent Development
---

When multiple AI agents need to work on the same monorepo simultaneously, they each need an isolated working copy with fully functional `node_modules`. Git worktrees combined with pnpm's [global virtual store](./global-virtual-store.md) make this practical: each worktree gets its own checkout and its own `node_modules`, but dependencies are shared across all of them through a single content-addressable store on disk.

## What is a git worktree?

Normally, a git repository has a single working directory tied to one branch at a time. If you want to look at another branch, you have to stash or commit your changes and switch. A [git worktree](https://git-scm.com/docs/git-worktree) lets you check out multiple branches simultaneously, each in its own directory. All worktrees share the same repository history and objects — they're just different views into the same repo.

```
git worktree add ../feature-branch feat/my-feature
```

This creates a new directory `../feature-branch` with `feat/my-feature` checked out, while your original working directory stays on its current branch. You can work in both directories independently.

A common pattern is to use a **bare repository** (one with no working directory of its own) as the hub, and create all working directories as worktrees:

```
git clone --bare https://github.com/your-org/your-repo.git your-repo
cd your-repo
git worktree add ./main main
git worktree add ./feature feat/something
```

## Why worktrees?

Even before AI agents, worktrees are useful for maintaining multiple major versions of a project. On my dev machine, I use a pnpm repository with at least two worktrees: one on `main` for pnpm v11, and another on the `v10` branch for backports and maintenance releases. This way, I can fix a bug on v10 without stashing my in-progress v11 work — both versions are always checked out and ready to go. In the past, 2 or 3 worktrees were usually enough for me in the pnpm repository. However, since I started using AI agents extensively, I need a lot more worktrees to let my agents work on many tasks in parallel.

## Why this matters even more with AI agents

With AI coding agents, worktrees go from convenient to essential. Each agent needs its own working directory to edit files, run builds, and execute tests without interfering with other agents. Without worktrees, this means cloning the repository multiple times, duplicating git history for each copy.

Worktrees solve the git side — every agent gets its own isolated checkout while sharing the underlying git objects. But each worktree still needs its own `node_modules`, which can be hundreds of megabytes. That's where pnpm's [global virtual store](./global-virtual-store.md) comes in: with it enabled, each worktree's `node_modules` contains only symlinks into a single content-addressable store on disk. This means adding a new agent is fast and costs almost no extra disk space.

:::important

This setup assumes the worktrees and agents share the same trust boundary. Do not use one writable pnpm store for mutually untrusted agents or users.

:::

## Setting it up

### 1. Create a bare repository

```sh
git clone --bare https://github.com/your-org/your-monorepo.git your-monorepo
cd your-monorepo
```

### 2. Create worktrees for each branch

```sh
# Main development worktree
git worktree add ./main main

# A feature branch for agent A
git worktree add ./feature-auth feat/auth

# A bugfix branch for agent B
git worktree add ./fix-api fix/api-error
```

Each worktree is a full checkout with its own files, but they all share the same `.git` object store.

### 3. Enable the global virtual store

Add `virtualStoreType: global` to the `pnpm-workspace.yaml` in your repository (before v11.23.0 this setting was spelled `enableGlobalVirtualStore: true`):

```yaml
packages:
  - 'packages/*'

virtualStoreType: global
```

### 4. Install dependencies in each worktree

```sh
cd main && pnpm install
cd ../feature-auth && pnpm install
cd ../fix-api && pnpm install
```

The first `pnpm install` downloads packages into the global store. Subsequent installs in other worktrees are nearly instant because they only create symlinks to the same store.

## How it works

Without the global virtual store, each worktree would have its own `.pnpm` virtual store inside `node_modules`, with hardlinks or copies of every package. With `virtualStoreType: global`, pnpm keeps all package contents in a single shared directory (the global store, which you can find by running `pnpm store path`), and each worktree's `node_modules` contains symlinks pointing there:

```
your-monorepo/                      (bare git repo)
├── main/                           (worktree: main branch)
│   ├── packages/
│   └── node_modules/
│       ├── lodash → <global-store>/links/@/lodash/...
│       └── express → <global-store>/links/@/express/...
├── feature-auth/                   (worktree: feat/auth branch)
│   └── node_modules/
│       ├── lodash → <global-store>/links/@/lodash/...  ← same target
│       └── express → <global-store>/links/@/express/...
└── fix-api/                        (worktree: fix/api-error branch)
    └── node_modules/
        ├── lodash → <global-store>/links/@/lodash/...  ← same target
        └── express → <global-store>/links/@/express/...
```

This means:
- **Near-zero per-worktree overhead** — the local `node_modules` contains only symlinks to the shared global virtual store. Unlike pnpm's default behavior, which hardlinks files from the content-addressable store into a local `node_modules/.pnpm` directory, the global virtual store means no files are copied or hardlinked into the worktree at all.
- **Instant installs for new worktrees** — packages are already in the global store.
- **No conflicts** — each worktree has its own `node_modules` tree, so agents can install different dependency versions on different branches without interference.

## Example: the pnpm monorepo itself

The [pnpm repository](https://github.com/pnpm/pnpm) uses this exact setup with a bare git repo and a global virtual store. It includes helper scripts to make worktree management easier:

**`pnpm worktree:new <branch-name|pr-number>`** — creates a new worktree and sets it up:

```sh
# Create a worktree for a branch (creates it from main if it doesn't exist)
pnpm worktree:new feat/my-feature

# Create a worktree for a GitHub PR (fetches the PR ref automatically)
pnpm worktree:new 10834
```

The script handles a few things beyond plain `git worktree add`:
- PR numbers are fetched via `git fetch origin pull/<number>/head` so they work for forks too.
- Branch names with slashes (e.g. `feat/my-feature`) are converted to dashes for the directory name (e.g. `feat-my-feature`).
- The `.claude` directory is symlinked from the bare repo's git common directory into the new worktree, so all worktrees share the same Claude Code settings and approved commands.

There's also a shell helper [`shell/wt.sh`](https://github.com/pnpm/pnpm/blob/main/shell/wt.sh) that wraps the script and `cd`s into the new worktree:

```sh
# Source it in your shell config, then:
wt feat/my-feature
wt 10834
```

## Tips

- **Creating worktrees for agents**: When launching an AI agent, create a dedicated worktree for it. The agent gets full isolation to edit files, run tests, and install packages without affecting other agents.
- **Cleanup**: Remove a worktree when it's no longer needed with `git worktree remove ./feature-auth`. Leftover worktrees are cheap but can accumulate.
