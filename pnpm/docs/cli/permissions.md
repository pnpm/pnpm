---
id: permissions
title: pnpm permissions
---

Added in: v12.11.0

Review what dependencies may do: run build scripts (`build`) and provide [agent skills](../agent-skills.md) (`skills`). The decisions are stored in the [`permissions`] setting of `pnpm-workspace.yaml`.

[`permissions`]: ../settings/build.md#permissions

## Commands

### pnpm permissions approve

Alias: `pnpm approve`

Approve or deny what the dependencies awaiting approval request. An install that skips a package's build scripts or skills records it as awaiting approval, so run `pnpm install` first. A package appears once, with everything it requests:

```
drizzle-kit  build, skills
esbuild      build
```

Without package arguments or `--all`, the packages are chosen in an interactive prompt. In CI or without an interactive terminal, running `pnpm approve` without package arguments or `--all` fails with `ERR_PNPM_APPROVE_NON_INTERACTIVE` when packages are awaiting approval. The packages you do not select are denied. For a package you approve that ships several skills, a follow-up prompt lists them and you choose which ones to link. A skill you leave out is neither linked nor offered again: the recorded list names the only skills pnpm may link. When your project approves skills for the first time, pnpm also asks which agent skill directories to link into, and records your choices as [`skills.dirs`](../settings/build.md#skillsdirs). Leave every directory unselected and the detected directories decide instead.

You can also pass package names, prefixing a name with `!` to deny it:

```sh
pnpm approve drizzle-kit !esbuild
```

A named package is approved or denied for everything it requests, including every skill it ships. If a named package has no pending request, pnpm asks which permissions to grant in advance. Each permission starts unchecked. Leaving one unchecked preserves its current setting. Cancelling the prompt saves no decisions from the command.

In CI or without an interactive terminal, naming a package to approve with no pending request fails before any decisions are saved. Set the package's [`permissions`] explicitly for unattended use, or use [`pnpm approve-builds`](./approve-builds.md) for a build decision. An explicit denial (`!<pkg>`) with no pending request still denies every capability.

Each Git package appears once per repository. Press **Space** to approve its pending commits, or **R** to include future commits. Press **Enter** to save. To approve a single revision, pass its full commit selector to `pnpm approve`. `--all` approves only pending commits.

To approve a Git package across updates without a prompt, pass its package name and exact `git+` repository URL without a commit or branch suffix. For a `github:` dependency, the URL ends in `.git`. See the [repository matching rules](../settings/build.md#allowbuilds).

```sh
pnpm approve 'example@git+https://github.com/example/project.git'
```

This approves the requested capabilities for any commit from that repository. If a bare name or branch selector cannot match a pending Git package, pnpm lists the valid selectors and saves no decisions.

After the decision is written, pnpm rebuilds the packages approved to build and links the approved agent skills.

A `build` decision is written to `permissions` only when `pnpm-workspace.yaml` already has a `permissions` setting. Otherwise it is written to [`allowBuilds`](../settings/build.md#allowbuilds), which older pnpm versions read too.

#### --all

Approve everything awaiting approval without interactive prompts.

#### --global, -g

Approve permissions for [globally installed packages](../global-packages.md).

pnpm gathers the packages awaiting approval across every [isolated install
group](../global-packages.md#isolated-installations), asks about them once, and
writes the decisions to [`globalPackagesConfig.permissions`](../configuring.md#settings-for-global-packages)
in the global `config.yaml`. Skill directory choices are written to
`globalPackagesConfig.skills.dirs`. Only the groups that actually contain an
approved package are then rebuilt, and approved skills are linked into agent
skill directories under your home directory.

### pnpm permissions list

Alias: `pnpm permissions`, `pnpm permissions ls`

List the granted and denied permissions, and the packages awaiting approval.
