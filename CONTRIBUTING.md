# Contributing

## Table of contents

- [Setting Up the Environment](#setting-up-the-environment)
- [Working with Git Worktrees](#working-with-git-worktrees)
- [Running Tests](#running-tests)
- [Submitting a Pull Request (PR)](#submitting-a-pull-request-pr)
  - [After your pull request is merged](#after-your-pull-request-is-merged)
- [Coding Style Guidelines](#coding-style-guidelines)
- [Commit Message Guidelines](#commit-message-guidelines)
  - [Commit Message Format](#commit-message-format)
    - [Revert](#revert)
    - [Type](#type)
    - [Scope](#scope)
    - [Subject](#subject)
    - [Body](#body)
    - [Footer](#footer)

## Setting Up the Environment

1. Run `pnpm install` in the root of the repository to install all dependencies.
1. Run `pnpm add ./pnpm/dev -g` to make pnpm from the repository available in the command line via the `pd` command.
1. Run `pnpm run compile` to create an initial build of pnpm from the source in the repository.
1. Now you can change any source code file and run `pd [command] [flags]` to run `pnpm` directly from the source code by compiling all the files without typechecking in memory.
1. Alternatively, for recompiling all the projects with typechecking after your changes, again run `pnpm run compile` in the root of the repository.
1. In order to run all the tests in the repository, run `pnpm run test-main`. You may also run tests of specific projects by running `pnpm test` inside a project's directory or using `pnpm --filter <project name> test`.

Some of the e2e tests run node-gyp, so you might need to install some build-essentials on your system for those tests to pass. On Fedora, install these:

```shell
sudo dnf install make automake gcc gcc-c++ kernel-devel
```

## Working with Git Worktrees

Worktrees let you check out multiple branches simultaneously in separate directories,
which is useful for working on several issues in parallel without stashing or switching branches.
This is particularly powerful when running multiple AI coding agents (e.g. Claude Code) at the
same time — each agent gets its own isolated worktree, so they can work concurrently without
interfering with each other.

### Setting up a bare-repo layout with worktrees

Cloning as a **bare** repository lets all worktrees live as children of a single top-level
directory. That avoids the "one privileged main clone + siblings" asymmetry: every branch is
just a directory next to the others, and there is no working tree attached to the bare repo
itself. This is especially handy when you expect to keep many worktrees around long-term —
for example, one per in-flight PR, or one per parallel AI agent.

The resulting layout looks like this:

```
~/src/pnpm/pnpm/          # the bare repo (contains HEAD, config, objects/, refs/, worktrees/)
├── main/                 # worktree for main
├── v10/                  # worktree for the v10 release branch
├── fix-1234/             # worktree for branch fix/1234
└── feat-my-feature/      # worktree for branch feat/my-feature
```

One-time setup:

1. Clone as a bare repository at the directory that will hold all worktrees:

   ```shell
   git clone --bare https://github.com/pnpm/pnpm.git ~/src/pnpm/pnpm
   cd ~/src/pnpm/pnpm
   ```

2. Point Husky at a path that exists inside every worktree (not inside the bare repo's gitdir),
   so commit and push hooks run when you commit from any worktree:

   ```shell
   git config core.hooksPath .husky/_
   ```

3. Create the first worktree for `main` and install dependencies:

   ```shell
   git worktree add main main
   cd main
   pnpm install
   ```

4. Install [`@zkochan/git-wt`](https://github.com/zkochan/git-wt) globally. It provides a
   `git-wt` binary that creates a worktree for a branch or PR and prints its path, plus a
   `wt` shell function that `cd`s into the new worktree in one step:

   ```shell
   pnpm add -g @zkochan/git-wt
   ```

   Then wire the `wt` function into your shell config so it's available in every future
   session. Pick the snippet for your shell — it appends to the right rc file and activates
   `wt` in the current session too:

   **fish**:

   ```shell
   echo 'git-wt init fish | source' >> ~/.config/fish/config.fish
   git-wt init fish | source
   ```

   **bash**:

   ```shell
   echo 'eval "$(git-wt init bash)"' >> ~/.bashrc
   eval "$(git-wt init bash)"
   ```

   **zsh**:

   ```shell
   echo 'eval "$(git-wt init zsh)"' >> ~/.zshrc
   eval "$(git-wt init zsh)"
   ```

5. (Optional) If you push to your own fork as well as `origin`, add it once in the bare repo:

   ```shell
   git -C ~/src/pnpm/pnpm remote add <your-username> git@github.com:<your-username>/pnpm.git
   ```

### Usage

From inside any existing worktree:

```shell
# Create a worktree for an existing branch and switch to it
wt fix/4444

# Create a worktree for a new branch (branched from main) and switch to it
wt feat/my-feature

# Create a worktree for a GitHub PR (works for forks too) and switch to it
wt 10000
```

`wt` creates the new worktree next to the current one — in the bare-repo layout that means it
lands as a sibling of `main/`, inside the bare repo directory. Branch names with slashes get
their slashes replaced with dashes in the directory name (so `feat/my-feature` becomes
`feat-my-feature/`).

Passing a number is interpreted as a PR number. The PR is fetched via
`git fetch origin pull/<number>/head` into a local branch named `pr-<number>`, so it works
for both same-repo branches and forks.

If [Claude Code](https://www.anthropic.com/claude-code) is installed on your system, `wt
<pr-number>` will additionally launch a Claude review of the PR via the tracked hook at
`.git-wt/pr-hook`. The hook silently no-ops if `claude` isn't on your `PATH`, so contributors
who don't use Claude aren't affected. Requires `@zkochan/git-wt` ≥ 0.0.3, which is the
version that introduced the per-repo hook lookup.

If you only need the worktree path (e.g. to open it in an editor) without switching directories,
invoke `git-wt` directly — it's also exposed as a native git subcommand:

```shell
git wt feat/my-feature
git wt 10000
```

## Running Tests

You can run the tests of the project that you modified by going to the project's directory and running:

```shell
pnpm test
```

Alternatively, you can run it from anywhere by specifying the name of the project using the `--filter` option:

```shell
pnpm --filter core test
```

If you want to pass options to Jest, use the `pnpm run test` command and append any needed options. For instance, if you want to run a single test in a single file, run:

```shell
pnpm --filter core run test test/lockfile.ts -t "lockfile has dev deps even when installing for prod only"
```

## Submitting a Pull Request (PR)

Before you submit your Pull Request (PR) consider the following guidelines:

- Search [GitHub](https://github.com/pnpm/pnpm/pulls) for an open or closed PR
  that relates to your submission. You don't want to duplicate effort.
- Make your changes in a new git branch:

  ```shell
  git checkout -b my-fix-branch main
  ```

- Create your patch, following [code style guidelines](#coding-style-guidelines), and **including appropriate test cases**.
- Run `pnpm changeset` in the root of the repository and describe your changes. The resulting files should be committed as they will be used during release.
- Run the full test suite and ensure that all tests pass.
- Commit your changes using a descriptive commit message that follows our
  [commit message conventions](#commit-message-guidelines). Adherence to these conventions
  is necessary because release notes are automatically generated from these messages.

  ```shell
  git commit -a
  ```

  Note: the optional commit `-a` command line option will automatically "add" and "rm" edited files.

- Push your branch to GitHub:

  ```shell
  git push origin my-fix-branch
  ```

- In GitHub, send a pull request to `pnpm:main`.
- If we suggest changes then:

  - Make the required updates.
  - Re-run the test suites to ensure tests are still passing.
  - Rebase your branch and force push to your GitHub repository (this will update your Pull Request):

    ```shell
    git rebase main -i
    git push -f
    ```

That's it! Thank you for your contribution!

### After your pull request is merged

After your pull request is merged, you can safely delete your branch and pull the changes
from the main (upstream) repository:

- Delete the remote branch on GitHub either through the GitHub web UI or your local shell as follows:

  ```shell
  git push origin --delete my-fix-branch
  ```

- Check out the main branch:

  ```shell
  git checkout main -f
  ```

- Delete the local branch:

  ```shell
  git branch -D my-fix-branch
  ```

- Update your main with the latest upstream version:

  ```shell
  git pull --ff upstream main
  ```

## Coding Style Guidelines

[![js-standard-style](https://raw.githubusercontent.com/standard/standard/master/badge.svg)](https://github.com/standard/standard)

Use the [Standard Style](https://github.com/standard/standard).

## Commit Message Guidelines

[![Commitizen friendly](https://img.shields.io/badge/commitizen-friendly-brightgreen.svg)](http://commitizen.github.io/cz-cli/)

We have very precise rules over how our git commit messages can be formatted. This leads to **more
readable messages** that are easy to follow when looking through the **project history**.

### Commit Message Format

Each commit message consists of a **header**, a **body** and a **footer**.  The header has a special
format that includes a **type**, a **scope** and a **subject**:

    <type>(<scope>): <subject>
    <BLANK LINE>
    <body>
    <BLANK LINE>
    <footer>

The **header** is mandatory and the **scope** of the header is optional.

Any line of the commit message cannot be longer than 100 characters! This allows the message to be easier
to read on GitHub as well as in various git tools.

#### Revert

If the commit reverts a previous commit, it should begin with `revert:`, followed by the header of the reverted commit. In the body it should say: `This reverts commit <hash>.`, where the hash is the SHA of the commit being reverted.

#### Type

Must be one of the following:

- **feat**: A new feature
- **fix**: A bug fix
- **docs**: Documentation only changes
- **style**: Changes that do not affect the meaning of the code (white-space, formatting, missing
  semi-colons, etc)
- **refactor**: A code change that neither fixes a bug nor adds a feature
- **perf**: A code change that improves performance
- **test**: Adding missing tests
- **chore**: Changes to the build process or auxiliary tools and libraries such as documentation
  generation

#### Scope

The scope could be anything specifying place of the commit change. For example
`plugin-example`, `render-md`, etc.

#### Subject

The subject contains succinct description of the change:

- use the imperative, present tense: "change" not "changed" nor "changes"
- don't capitalize first letter
- no dot (.) at the end

#### Body

Just as in the **subject**, use the imperative, present tense: "change" not "changed" nor "changes".
The body should include the motivation for the change and contrast this with previous behavior.

#### Footer

The footer should contain any information about **Breaking Changes** and is also the place to
reference GitHub issues that this commit **Closes**.

**Breaking Changes** should start with the word `BREAKING CHANGE:` with a space or two newlines. The rest of the commit message is then used for this.
