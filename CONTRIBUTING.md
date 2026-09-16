# Contributing

## Table of contents

- [Setting Up the Environment](#setting-up-the-environment)
  - [JavaScript and TypeScript CLI](#javascript-and-typescript-cli)
  - [Rust toolchain and git hooks](#rust-toolchain-and-git-hooks)
- [Working with Git Worktrees](#working-with-git-worktrees)
- [Running Tests](#running-tests)
- [Submitting a Pull Request (PR)](#submitting-a-pull-request-pr)
  - [AI-assisted contributions](#ai-assisted-contributions)
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

The repository holds two implementations of the same package manager: the TypeScript pnpm CLI and the Rust `pacquet` port (plus the Rust `pnpr` registry server). Most contributions touch Rust, but the two stacks share one workspace, so set up both.

### JavaScript and TypeScript CLI

1. Install pnpm using one of the [official installation methods](https://pnpm.io/installation). **Do not use Corepack.** The scripts in this repository invoke pnpm through the `pn` and `pnx` aliases, which the official installation methods create. Corepack only provides the `pnpm` and `pnpx` commands, so with a Corepack-managed pnpm the build fails with errors like `pn: Permission denied` ([pnpm/pnpm#12448](https://github.com/pnpm/pnpm/issues/12448)).
1. Set up the Rust toolchain first, as described under [Rust toolchain and git hooks](#rust-toolchain-and-git-hooks). `pnpm install` also installs the Rust dependencies and runs `cargo`, so it fails when `cargo` is not on `PATH`.
1. Run `pnpm install` in the root of the repository to install all dependencies.
1. Run `pnpm add ./pnpm11/pnpm/dev -g` to make pnpm from the repository available in the command line via the `pd` command.
1. Run `pnpm run compile` to create an initial build of pnpm from the source in the repository.
1. Now you can change any source code file and run `pd [command] [flags]` to run `pnpm` directly from the source code by compiling all the files without typechecking in memory.
1. Alternatively, for recompiling all the projects with typechecking after your changes, again run `pnpm run compile` in the root of the repository.
1. Run the tests for what you changed rather than the whole repository, which takes a long time: `pnpm test` inside a project's directory, `pnpm --filter <project name> test` from the root, or `pnpm --filter <project name> test <file path>` for a single file. `pnpm run test-all` runs the whole TypeScript suite on the rare occasion you need it; the Rust and `pnpr` suites are separate, and are covered in [`pnpm/CONTRIBUTING.md`](./pnpm/CONTRIBUTING.md).

Some of the e2e tests run node-gyp, so you might need to install some build-essentials on your system for those tests to pass. On Fedora, install these:

```shell
sudo dnf install make automake gcc gcc-c++ kernel-devel
```

### Rust toolchain and git hooks

Rust is now the primary language in this repository, so most contributions need a working Rust toolchain and the Rust developer tools. The Rust workspace (`Cargo.toml`, `rust-toolchain.toml`, `justfile`) lives at the repository root; run `cargo` and `just` from there.

1. Install [`rustup`](https://rustup.rs). You do not need to select a toolchain by hand. `rust-toolchain.toml` pins the version the project builds with, and `rustup` installs it, together with `clippy` from the pinned `default` profile, the first time you run `cargo` inside the repository.

2. Install [`just`](https://just.systems) (the task runner) and [`cargo-binstall`](https://github.com/cargo-bins/cargo-binstall), then install the task tools from the repository root:

   ```shell
   just init
   ```

   `just init` installs `cargo-nextest`, `cargo-watch`, `cargo-insta`, `typos-cli`, `taplo-cli`, `wasm-pack`, and `cargo-llvm-cov` (via `cargo binstall`), plus `cargo-fixit` (pinned to `0.1.15` via `cargo install cargo-fixit@0.1.15 --locked`, since `cargo-fixit` has no prebuilt binaries). `cargo-fixit` backs the `just fix` task. It also builds the pinned pnpm rustfmt fork described below.

   Because `cargo-fixit` is built from source, it needs a complete OpenSSL development installation — headers and libraries — which its `openssl-sys` dependency looks for at build time. Without one, `just init` fails while installing `cargo-fixit`, before it reaches the rustfmt step, with `failed to run custom build command for openssl-sys`. Install it first: `sudo dnf install openssl-devel` on Fedora, `sudo apt install libssl-dev pkg-config` on Debian and Ubuntu. If OpenSSL is installed somewhere `openssl-sys` does not look — under a Homebrew prefix, for instance — point it at that existing installation with `OPENSSL_DIR`, for example `export OPENSSL_DIR=$(brew --prefix openssl@3)`. Note that `OPENSSL_DIR` only locates an installation; it is not a substitute for one.

3. Install the dylint tools, which `just init` does not cover, **from source**:

   ```shell
   cargo install cargo-dylint dylint-link
   ```

   Install these from source rather than with `cargo binstall`. The prebuilt `cargo-dylint` binaries reference the `dylint_driver` crate at the path where they were built, so building the per-toolchain driver fails locally with an error that points at a nonexistent `.../dylint/driver` directory. A `cargo install` build resolves the driver against your local cargo registry and works.

`pnpm install` at the repository root installs the Rust dependencies alongside the JavaScript ones. It reads `Cargo.lock`, links the registry crates into `.pnpm/crates/crates-io` and the git-sourced ones into `.pnpm/crates/git`, and points Cargo at both through the source replacement block committed in `.cargo/config.toml`. `cargo build` and `cargo test` work as before, and `cargo metadata --locked --offline` confirms that every dependency resolves without network access.

Every crate reaches the build this way, in CI as well as locally. Cargo resolves nothing from its own registry, so a cargo command needs an install behind it. Run `pnpm install` after a fresh clone, and again after any checkout that moves `Cargo.lock`, a `git bisect` step included. Until you do, `cargo build` fails with `failed to load source for dependency`. Add crates with `pnpm add`: `cargo add` resolves against the replaced source and cannot find a crate that is not already installed.

The block is a function of `Cargo.lock`, so an install regenerates it byte for byte and leaves `git status` clean. It changes only when a git-sourced dependency moves to a new revision, and that change belongs in the same commit as the lockfile. Rust CI fails if the two drift apart.

Make sure `~/.cargo/bin` is on your `PATH`, ahead of any system-wide Rust in `/usr/bin`. `rustup`'s installer adds this entry through `~/.cargo/env`; ensure your shell sources it. This matters for the git hooks. The `pnpm install` step above wires up husky, and its `pre-push` hook runs the Rust checks in `pnpm/scripts/pre-push-rust.sh` (format, doc, dylint, typos) alongside the TypeScript compile and lint. That script locates `cargo`, `rustup`, `taplo`, `typos`, and `cargo-dylint` through `PATH`, and it **skips** a check when the tool is not found rather than failing. A push that appears to pass locally with the tools off `PATH` has silently skipped the format, doc, and dylint checks, so those problems surface only in CI.

For the full Rust development workflow (checks, tests, benchmarks, and the code style guide), see [`pnpm/CONTRIBUTING.md`](./pnpm/CONTRIBUTING.md).

### Rust formatting

Use `just fmt` to format Rust and TOML, or run the Rust formatter directly:

```shell
node pnpm/scripts/rustfmt.mjs --all
node pnpm/scripts/rustfmt.mjs --all -- --check
```

The wrapper uses the [pnpm rustfmt fork](https://github.com/pnpm/rustfmt), pinned by full commit SHA in [`pnpm/scripts/rustfmt.json`](./pnpm/scripts/rustfmt.json). `use_small_heuristics = "Max"` keeps ordinary calls and short struct literals compact within the 100-column line limit.

With `chain_complexity_layout = true`, a chain of at most `chain_width = 40` columns of expression text stays inline when it fits the available line and head-width limits. Beyond that allowance, up to two method calls with simple arguments can stay inline if the expression fits the line. Zero-argument methods on simple receivers and short expression closures count as simple arguments. Longer chains and chains with complex arguments wrap vertically. These rules also apply in conditions.

In a vertical chain, keep the first method attached when the first line would otherwise end at or before the next indentation level, following the [Rust style guide](https://doc.rust-lang.org/style-guide/expressions.html#chains-of-fields-and-method-calls). The allowance is `tab_spaces` (normally 4 columns), counting the receiver and any preceding code such as `let value =` or `if`, but excluding the enclosing block indentation. Moving an assignment’s right-hand side to a new line does not create a new allowance. This does not attach a second method or apply to a receiver that is already a function call. A single method stays attached to a simple receiver, including when its arguments span multiple lines. After a function-call receiver, the method moves to its own line if attaching it would split its arguments.

Leading field accesses stay with their receiver while the prefix fits within `chain_head_width = 80` columns, including indentation and preceding code such as `let value =`. An intermediate access that exceeds that limit starts a new line. A final access uses the normal 100-column limit. Fields after a vertically wrapped method each start a new line. `.await` follows the same layout rules as ordinary fields. `?` stays attached to the preceding expression.

Destructuring uses `struct_pattern_width = 35`, keeping short patterns such as `EnvSubcommand::Use { package_name }` inline.

Ordinary `cargo fmt` uses the toolchain's upstream formatter and does not apply this rule. The task runner, CI, and git hook all use the wrapper.

The first run installs the formatter's dated nightly toolchain with `rustc-dev` and LLVM tools, then builds the two formatter binaries with `cargo install --locked`. This needs network access, disk space for the compiler components, and the platform's Rust build prerequisites. Run `node pnpm/scripts/rustfmt.mjs --install` ahead of time to prepare the formatter. The project still builds with the stable compiler in `rust-toolchain.toml`; the formatter has its own runtime and does not replace any global binaries.

Binaries are cached under `~/.cache/pnpm/rustfmt`, keyed by OS, architecture, nightly toolchain, and fork revision. `node pnpm/scripts/rustfmt.mjs --cache-path` prints the exact installation directory. Cached runs work offline when the required toolchain components are already installed. Updating the revision in `rustfmt.json` causes the next run to build a new cached copy. Installation or formatting failures are reported as errors; the wrapper never falls back to another formatter.

For editors, `--rustfmt` forwards arguments directly to the pinned formatter and preserves its stdin/stdout interface. In VS Code with rust-analyzer, set [`rust-analyzer.rustfmt.overrideCommand`](https://rust-analyzer.github.io/book/configuration#rust-analyzer.rustfmt.overrideCommand) in your workspace settings:

```json
{
  "rust-analyzer.rustfmt.overrideCommand": [
    "node",
    "/absolute/path/to/checkout/pnpm/scripts/rustfmt.mjs",
    "--rustfmt",
    "--edition=2024",
    "--config-path",
    "/absolute/path/to/checkout/rustfmt.toml"
  ]
}
```

Replace the two paths with your checkout paths. On Windows, use forward slashes or escape backslashes in JSON. Other editors can run the same command with source code on stdin. Install the formatter before enabling format-on-save to avoid the initial build delaying an editor request.

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

For PR worktrees, `git wt <pr-number>` will additionally launch an agent review of the PR via
the tracked hook at `.git-wt/pr-hook`. The hook defaults to
[Claude Code](https://www.anthropic.com/claude-code), but you can pass another agent CLI name
after the PR number, for example `git wt 10000 codex`. The hook silently no-ops if the
requested CLI isn't on your `PATH`, so contributors who don't use an agent aren't affected.
Requires a version of `@zkochan/git-wt` with per-repo hook lookup.

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

- Check whether the issue you are fixing already has a PR. GitHub automatically
  cross-links every PR that references an issue on the issue's timeline, so open
  the issue and look at its linked pull requests. If a PR already solves the
  issue, contribute by reviewing or improving that PR instead of opening a
  competing one — duplicate PRs are closed in favor of the first viable one, and
  the effort spent on them (yours and the reviewers') is wasted.
- Make your changes in a new git branch:

  ```shell
  git checkout -b my-fix-branch main
  ```

- Create your patch, following [code style guidelines](#coding-style-guidelines), and **including appropriate test cases**.
- Run `pnpm change` in the root of the repository and describe your changes. The resulting files should be committed as they will be used during release. Write the description for pnpm users and keep it concise — it becomes a release note. Implementation rationale belongs in the commit message, not the changeset. The wording rules are in [Changeset style](AGENTS.md#changeset-style).
- Run the tests that cover your change and ensure they pass, along with the
  linters. You do not need to run the whole suite locally: CI runs it on every
  pull request. For the Rust workspace, see
  [`pnpm/CONTRIBUTING.md`](pnpm/CONTRIBUTING.md#automated-checks).
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
- Wait for the automated reviewers. A human reviewer will only start the review
  process once CodeRabbit has approved the PR and CI is green, so address its
  findings first.
- If we suggest changes then:

  - Make the required updates.
  - Re-run the tests that cover the updated code to ensure they still pass.
  - Rebase your branch and force push to your GitHub repository (this will update your Pull Request):

    ```shell
    git rebase main -i
    git push -f
    ```

That's it! Thank you for your contribution!

### AI-assisted contributions

We use AI coding agents ourselves and welcome contributions made with them. But
you, the contributor, are responsible for what you submit — an agent's output is
a draft, not a finished PR. Maintainer review time is the scarcest resource this
project has, and a stream of unvetted agent-generated PRs consumes it faster
than any other kind of contribution. Before submitting, make sure that:

- You checked the issue's linked PRs and are not duplicating an existing fix.
  Agents will happily produce a patch for an issue that is already solved.
- You understand the change and can answer review questions about it yourself.
- You ran the relevant tests locally and they pass.
- The PR does only what it says: no drive-by reformatting, unrelated fixes, or
  invented refactors padding the diff.
- Agent-written PRs, issues, and comments disclose it with a footer naming the
  agent and the model, e.g.
  `Written by an agent (Claude Code, claude-opus-4-7).`

PRs that appear to be unreviewed agent output — duplicating an existing PR,
failing to compile, or not addressing the referenced issue — may be closed
without detailed review.

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
