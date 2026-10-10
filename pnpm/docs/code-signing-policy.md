---
id: code-signing-policy
title: Code signing policy
---

Free code signing provided by [SignPath.io](https://about.signpath.io/), certificate by [SignPath Foundation](https://signpath.org/).

The Windows executables, `pnpm.exe` for x64 and arm64, are signed. They are built from the [pnpm/pnpm](https://github.com/pnpm/pnpm) repository by the release workflow on GitHub Actions, and each release is signed only after a maintainer approves it.

## Team roles

* Committers and reviewers: [members of the pnpm organization](https://github.com/orgs/pnpm/people)
* Approvers: [Zoltan Kochan](https://github.com/zkochan)

## Privacy policy

This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it.

pnpm contacts the package registries and other sources that your project and configuration name. `pnpm install` and `pnpm add` also ask the configured registry for the latest pnpm version at most once a day. Set [`updateNotifier`](./settings/other.md#updatenotifier) to `false` to turn this off.
