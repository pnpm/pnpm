# Agent Guide to pnpm Repository

This document provides context and instructions for AI agents working on the pnpm codebase.

The repository contains three products:

- The **TypeScript pnpm v11 CLI** — `pnpm11/`.
- The **Rust pnpm v12 CLI (pacquet)** — `pnpm/`. pnpm v12 is the target for new feature development. See [`pnpm/AGENTS.md`](./pnpm/AGENTS.md) for pacquet-specific rules; it adds to (and never contradicts) the conventions below.
- The **Rust pnpr registry server** — `pnpr/`. See [`pnpr/AGENTS.md`](./pnpr/AGENTS.md) for pnpr-specific rules; it adds to (and never contradicts) the conventions below.

Sections below marked "(TypeScript only)" apply to TypeScript code only; they do not apply to Rust code in `pnpm/` or `pnpr/`. Everything else applies repo-wide unless a nested `AGENTS.md` specializes it.

## pnpm v12 and v11 development policy

pnpm v12, implemented in Rust under `pnpm/`, is the target for new development. pnpm v11, implemented in TypeScript under `pnpm11/`, is maintained for bug fixes.

**Implement new features only in pnpm v12. Do not add them to pnpm v11.** A feature that intentionally exists only in v12 is not a parity gap.

For bug fixes, first determine which versions contain the bug. If the bug is present in both v11 and v12, implement and test the fix in both stacks. Keep their observable behavior aligned for the affected functionality, including command-line flags and defaults, environment-variable handling, lockfile/manifest/state-file formats, error codes and messages, log emissions parsed by `@pnpm/cli.default-reporter`, store layout, and hook semantics. If the bug exists in only one version, fix only that version.

When a shared bug fix cannot be completed in both stacks in the same PR, call out the missing implementation in the PR description so it can be added before the PR lands.

The pacquet-side version policy is in [`pnpm/AGENTS.md`](./pnpm/AGENTS.md#version-policy).

## Repository Structure

The pnpm codebase is a monorepo managed by pnpm itself. The root contains functional directories organized by domain:

### TypeScript pnpm v11 Core Directories

-   `pnpm11/pnpm/`: The CLI entry point and main package.
-   `pnpm11/pkg-manager/`: Core package management logic (installation, linking, etc.).
-   `pnpm11/resolving/`: Dependency resolution logic (resolvers for npm, tarballs, git, etc.).
-   `pnpm11/fetching/`: Package fetching logic.
-   `pnpm11/store/`: Store management logic (content-addressable storage).
-   `pnpm11/lockfile/`: Lockfile handling, parsing, and utilities.

### CLI & Configuration

-   `pnpm11/cli/`: CLI command implementations and infrastructure.
-   `pnpm11/config/`: Configuration management and parsing.
-   `pnpm11/hooks/`: pnpm hooks (readPackage, etc.).
-   `pnpm11/cli/commands/src/completion/`: Shell completion support.

### Other Functional Directories

-   `pnpm11/network/`: Network-related utilities (proxy, fetch, auth).
-   `pnpm11/workspace/`: Workspace-related utilities.
-   `pnpm11/exec/`: Execution-related commands (run, exec, dlx).
-   `pnpm11/engine/runtime/commands/`: Node.js environment management.
-   `pnpm11/cache/`: Cache-related commands and utilities.
-   `pnpm11/patching/`: Package patching functionality.
-   `pnpm11/releasing/`: Release and publishing utilities.

### Shared Utilities

-   `pnpm11/fs/`: Filesystem utilities.
-   `pnpm11/crypto/`: Cryptographic utilities.
-   `pnpm11/text/`: Text processing utilities.

### Rust Projects

-   `pnpm/`: The Rust pnpm v12 CLI. Self-contained sub-project with its own crates, tests, and tooling — see [`pnpm/AGENTS.md`](./pnpm/AGENTS.md).
-   `pnpr/`: The pnpm-compatible npm registry server. Self-contained sub-project with its own crates, tests, and tooling — see [`pnpr/AGENTS.md`](./pnpr/AGENTS.md).

## Setup & Build (TypeScript only)

To set up the environment and build the project:

```bash
pnpm install
pnpm run compile
```

To compile a specific package:

```bash
pnpm --filter <package_name> run compile
```

**Important:** The TypeScript pnpm v11 CLI e2e tests (in `pnpm11/pnpm/test/`) use the **bundled** `pnpm11/pnpm/dist/pnpm.mjs`, not the individual package `lib/` outputs. After changing any TypeScript package, you must rebuild the bundle before running e2e tests:

```bash
pnpm --filter pnpm run compile
```

This runs `tsgo --build`, linting, and `pnpm run bundle` (which bundles all TypeScript packages into `pnpm11/pnpm/dist/pnpm.mjs`). Without this step, e2e tests will use a stale bundle and your changes won't be tested.

## Testing (TypeScript only)

Never run all tests in the repository as it takes a lot of time.

Run tests for a specific project instead:

```bash
# From the project directory
pnpm test

# From the root, filtering by package name
pnpm --filter <package_name> test
```

Or better yet, run tests for a specific file:

```bash
pnpm --filter <package_name> test <file_path>
```

Or a specific test case in a specific file:

```bash
pnpm --filter <package_name> test <file_path> -t <test_name_pattern>
```

## Linting (TypeScript only)

To run all linting checks:

```bash
pnpm run lint
```

## Never ignore test failures

Do not dismiss a failing test as a "pre-existing" failure that is unrelated to your changes. Every test failure must be investigated and fixed. If a test was already broken before your changes, fix it as part of your work — do not silently skip it or treat it as acceptable.

## AI Review Guidance

The repository's review framework lives in **[REVIEW_GUIDE.md](./REVIEW_GUIDE.md)** — how changes are accepted or rejected, the security-first / performance-second priorities, the security checklist and advisory regression themes, and the test/changeset/version-coverage expectations. Apply it when reviewing pull requests. (TypeScript-specific code style and engineering conventions for the CLI are documented in the "Code Style" section of this file; pacquet and pnpr follow their own `AGENTS.md` and style guides.)

Security is the first review priority and performance the second. Surface only issues tied to the changed code, and explain the exploit path, impact, or hot path affected. See the guide's Security and Performance review sections for the full checklist.

## Code Reuse and Avoiding Duplication

**Before writing new code, always analyze the existing codebase for similar functionality.** This is a large monorepo with many shared utilities — duplication is a real risk.

-   **Search before you write.** Before implementing any non-trivial logic, search the codebase for existing functions, utilities, or patterns that do the same or similar thing. Check `packages/`, `fs/`, `crypto/`, `text/`, and other shared directories first.
-   **Extract shared code.** If you find that the logic you need already exists in another package but is not exported or reusable, refactor it into a shared package rather than duplicating it. If you are adding new code that is similar to code that already exists elsewhere in the repo, move the common parts into a shared package that both locations can use.
-   **Prefer open source packages over custom implementations.** Do not reimplement functionality that is already available as a well-maintained open source package. Use established libraries for common tasks (e.g., path manipulation, string utilities, data structures, schema validation). Only write custom code when no suitable package exists or when the existing packages are too heavy or unmaintained.
-   **Keep the dependency on the right level.** When adding a new open source dependency, add it to the most specific package that needs it, not to the root or to a shared package unless multiple packages depend on it.

## Commit Messages

Follow the [Conventional Commits](https://www.conventionalcommits.org/) specification.

-   `feat`: a new feature
-   `fix`: a bug fix
-   `docs`: documentation only changes
-   `style`: formatting, missing semi-colons, etc.
-   `refactor`: code change that neither fixes a bug nor adds a feature
-   `perf`: a code change that improves performance
-   `test`: adding missing tests
-   `chore`: changes to build process or auxiliary tools

### Install the git hooks before committing

The git hooks in `.husky/` (including the `commit-msg` check described below) only run once husky has wired them into git. A fresh clone does **not** have them active until installed. **Before making any commit, ensure the hooks are installed** by running one of:

```bash
pnpm install      # runs the "prepare": "husky" script as part of install
# or, if dependencies are already installed, register the hooks on their own:
pnpm exec husky
```

You can confirm the hooks are active with `git config core.hooksPath` (it should point at husky's directory) and by checking that `.husky/_/` exists. Do not commit with hooks uninstalled — that silently skips every check, including the bare `#NNN` rejection below.

### Never use bare `#NNN` issue/PR references

**Do not write a bare `#NNN` (a `#` followed by digits) anywhere in a commit message.** A `commit-msg` hook (`.husky/reject-bare-issue-refs.mjs`) rejects them.

GitHub turns any `#NNN` into a link to issue/PR `NNN` of *this* repo, which is almost never what a bare reference means. This is a frequent AI mistake in two forms:

-   Using `#1`, `#2`, `#3`, … to enumerate items in a list. GitHub instead links them to unrelated issues `#1`, `#2`, `#3` of this repo. **Fix:** don't use `#` for enumeration — write `item 1`, `(1)`, `1.`, or rephrase.
-   Referring to issue `#NNN` of a *different* repository. GitHub instead links it to issue `NNN` of this repo. **Fix:** use qualified syntax `owner/repo#NNN` or an absolute URL `https://github.com/owner/repo/issues/NNN`.

For references to issues/PRs in **this** repo, also use the qualified form `pnpm/pnpm#NNN` or the absolute URL `https://github.com/pnpm/pnpm/issues/NNN`. Qualified syntax and absolute URLs are always unambiguous, so this rule is applied to every `#NNN` without exception.

**Address the root cause when the hook fires.** Rewrite the reference into the correct unambiguous form. Never bypass the check with `git commit --no-verify`, by editing or deleting the hook, or with any suppression file.

### Never use a bare `@mention`

**Do not write a bare `@name` (an `@` followed by a username-like token) anywhere in a commit message.** A `commit-msg` hook (`.husky/reject-bare-mentions.mjs`) rejects them.

GitHub turns any `@name` into a mention of that user/org/team, which is wrong either way it is meant:

-   If it is code (a scoped package like `@pnpm/core`, a handle, a path), GitHub should not treat it as a mention.
-   If it really is a person, every push, force-push, and rebase that carries the commit re-notifies them — noise nobody asked for.

**Fix:** wrap the reference in backticks so GitHub renders it as code and sends no notification — e.g. `` `@pnpm/core` `` or `` `@foo` `` — or remove it if it is not needed. Never bypass the check with `git commit --no-verify`, by editing or deleting the hook, or with any suppression file.

## Changesets

If your changes affect published packages, you MUST create a changeset file in the `.changeset` directory (`pnpm change` records one interactively; `pnpm change status` shows the pending release plan). The file describes the change and specifies the affected packages with their pending version bump types: patch, minor, or major. Write the description for pnpm users and keep it concise — it becomes a release note. Implementation rationale belongs in the commit message, not the changeset. The bare `pnpm version -r` consumes the pending changesets at release time; there is no separate `@changesets/cli` dependency.

**IMPORTANT: For changes to the TypeScript pnpm v11 CLI, always explicitly include `"pnpm"` in the changeset with a patch bump.** The changeset description will appear on the release notes page. For pnpm v12 changes, follow the Rust-product rules below and target `pacquet` instead.

Example:

```text
---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

Fixed a `pnpm install` bug that affected both pnpm v11 and v12.
```

The TypeScript pnpm v11 CLI is maintenance-only. Its changesets use patch bumps for bug fixes and internal maintenance. Do not implement new features or breaking changes in v11.

### Changeset style

A changeset is a release note. Someone skimming the changelog for the one entry that affects them should read it once and know what changed for them. Release blog posts are assembled from these entries, so their wording is the wording users see.

- **Lead with what the user sees.** The first sentence names the command, setting, or behavior that changed and how. If the mechanism matters to a user, give it a second sentence of its own. If it does not, leave it out. It belongs in the commit body.
- **One idea per sentence.** Split any sentence a reader would have to read twice. Do not chain several changes with colons, semicolons, or a parenthetical. A performance changeset says what got faster and for whom, not which caches and hash functions changed.
- **End with the issue link, not with the old behavior.** If the reader needs the old behavior to recognize the bug, describe it in its own past-tense sentence. Do not append "instead of ..." or "rather than ..." to every sentence.
- **Plain punctuation.** No em dashes or en dashes. End the sentence or use a comma. A colon only introduces a list or an example. No bold or italics for emphasis. Straight quotes.
- **Plain words.** "Use", not "leverage". "Many", not "numerous". "If", not "in the event that". Name the actor when it matters: "pnpm now reads the file", not "the file is now read". Replace "significantly faster" with the number, or drop the adverb.
- **No reasoning.** A changeset states what pnpm does now. Design justification, "so that ..." chains, "note that", and hedging go in the commit body or the PR description.

Before committing, reread the entry and ask what makes it read as generated. The usual answers are an em dash, an "instead of" tail, and a colon-joined list of internals.

Before:

```text
Sped up installs in large workspaces: the fast lockfile-update check no longer compares every project against every lockfile entry (or copies the whole lockfile before discovering a change needs the resolver), project ordering uses faster hashing, and the version-preference table builds in parallel [#14352](https://github.com/pnpm/pnpm/issues/14352).
```

After:

```text
Sped up installs in large workspaces. The check that decides whether the lockfile needs updating no longer compares every project against every lockfile entry [#14352](https://github.com/pnpm/pnpm/issues/14352).
```

Before:

```text
Fixed `pnpm run`, `pnpm exec`, `pnpm rebuild`, and the script shortcuts not loading the pnpmfile, so an `updateConfig` hook never applied to them [#14433](https://github.com/pnpm/pnpm/issues/14433). A hook's settings — `extraEnv` and `extraBinPaths` among them — now reach the scripts and commands these spawn, as they do on pnpm 11.
```

After:

```text
`pnpm run`, `pnpm exec`, `pnpm rebuild`, and the script shortcuts such as `pnpm test` now load the pnpmfile, so `updateConfig` hook settings such as `extraEnv` and `extraBinPaths` reach the scripts they spawn [#14433](https://github.com/pnpm/pnpm/issues/14433).
```

### Changesets for the Rust products

The Rust products are released through the same native flow. Their npm wrapper packages are workspace packages with committed versions, so a user-visible change to a Rust product needs a changeset too, targeting:

- `pacquet` — the Rust pnpm v12 CLI (published to npm as `pnpm` and `@pnpm/exe`; named `pacquet` in-repo so its name can't collide with the TypeScript CLI). `@pnpm/napi` is a `versioning.fixed` group with it and bumps with it automatically.
- `@pnpm/napi` — the Node.js addon bindings for the Rust engine.
- `@pnpm/pnpr` — the pnpr registry server (published as `@pnpm/pnpr` and its platform packages, plus the `ghcr.io/pnpm/pnpr` Docker image).

pnpm v12 and its NAPI addon release as stable versions on the main lane. pnpr releases on the `alpha` prerelease lane configured in `pnpm-workspace.yaml`; `pnpm lane main --filter …` graduates it to a stable version.

Do not add `"pnpm"` to a Rust-only changeset: in changesets, `pnpm` always means the TypeScript v11 CLI package. A changeset for a pnpm v12 feature or v12-only bug fix targets `pacquet` and omits `"pnpm"`. A shared bug fix that lands in both versions carries one changeset naming both the affected TypeScript packages (plus `"pnpm"`) and the Rust wrapper(s).

Use `pacquet` as the changeset package name, but use `pnpm` in its release-note prose and command examples (`pnpm add`, not `pacquet add`). The published Rust CLI's executable is `pnpm`; `pacquet` is only its in-repo package identifier.

## Comments

These conventions apply to the TypeScript pnpm CLI, pacquet, and pnpr. Product-specific `AGENTS.md` files may add language-specific rules, but they do not weaken this baseline.

Write code that explains itself. A reader should understand what a function does from its name, parameters, and types — not from prose above the call site.

Defaults:

-   **Do not write a comment** that restates what the code already says. If renaming a variable, splitting a helper, or moving a check to a more obvious place would carry the information, do that instead.
-   **Do not repeat documentation** at call sites that already lives on the callee. If the function has JSDoc, a Rust doc comment, or equivalent API documentation, the call site shouldn't re-explain what calling it does. Update the documentation once; let every call site benefit.
-   **Put a shared *why* in one place.** When the same rationale underlies several related functions — peers that delegate to a common helper, or a type and its methods — document it once at that common home and reference it from the rest, instead of re-deriving it in each. This is the call-site rule applied sideways across peers, not just upward to a callee.
-   **Documentation comments are for the item's contract** — preconditions, postconditions, edge cases, why the item exists. Not for re-narrating the body.
-   **Do not record past implementation shape, refactor history, or "the previous code did X" framing.** That's what `git log` and `git blame` are for. Describe the current contract — what the code is and what it guarantees — not what it replaced. Phrasings like "used to", "previously", "the original X", or a parenthetical naming a removed type belong in the commit message, not in the source.

Write a comment only when:

-   The reason for the code is non-obvious from reading it (a hidden invariant, a workaround for a known bug, a deliberate exception to the surrounding pattern).
-   The right name doesn't fit — e.g., a temporary technical constraint that's worth flagging but doesn't justify a new symbol.

Before adding a comment, ask: "Could I rename, restructure, or extract instead?" If yes, do that. The bar for prose-in-code is high; the bar for prose-that-restates-code is "don't."

## Code Style (TypeScript only)

This repository uses [Standard Style](https://github.com/standard/standard) with a few modifications:
-   **Trailing commas** are used.
-   **Functions are preferred** over classes.
-   **Functions are declared after they are used** (hoisting is relied upon).
-   **Functions should have no more than two or three arguments.** If a function needs more parameters, use a single options object instead.
-   **Import Order**:
    1.  Standard libraries (e.g., `fs`, `path`).
    2.  External dependencies (sorted alphabetically).
    3.  Relative imports.

To ensure your code adheres to the style guide, run:

```bash
pnpm run lint
```

### Conventions

Recurring engineering conventions in this codebase — the rules reviewers most often enforce:

-   **Errors.** Throw `PnpmError` (from `@pnpm/error`) for user-reachable errors — they are part of the UX and carry a stable code. Programmer-error, type-guard, and unreachable-branch errors stay plain `Error`. Never swallow errors; catch only the specific expected code (not "any error" when you meant `ENOENT`). Throw on impossible states rather than continuing. Error messages must carry context, e.g. the offending path.
-   **Naming.** Functions are verbs; types and fields are specific, not generic. Reuse existing terminology rather than inventing synonyms. File names follow the existing convention; rename a concept everywhere it appears.
-   **Reuse repo libraries.** Don't add a dependency, or hand-roll logic, for a job an existing repo utility or an already-present library does — search for it first. Deduplicate copy-pasted logic into a shared function or package.
-   **String parsing.** Prefer plain string operations over a custom regular expression. When the input needs structured parsing with backtracking, use the existing parser-combinator pattern (`object/property-path`).
-   **Dependency placement.** Shared infrastructure (the logger, etc.) is a peer dependency. (The narrowest-package rule is covered under "Code Reuse and Avoiding Duplication" above.)
-   **Config and layering.** Configurable values flow through `@pnpm/config` and reach commands via options — don't hardcode them (CLI options are camelCased automatically). Command handlers return data and let the CLI print it, which keeps them unit-testable. Don't add a wrapper function that adds nothing.
-   **Async and loops.** Prefer async fs and `async/await`; run independent work with `Promise.all`/`Promise.any` and `await` what must complete; hoist invariant work out of loops.

## Common Gotchas

### Error Type Checking in Jest (TypeScript only)

When checking if a caught error is an `Error` object, **do not use `instanceof Error`**. Jest runs tests in a VM context where `instanceof` checks can fail across realms.

Instead, use `util.types.isNativeError()`:

```typescript
import util from 'util'

try {
  // ... some operation
} catch (err: unknown) {
  // ❌ Wrong - may fail in Jest
  if (err instanceof Error && 'code' in err && err.code === 'ENOENT') {
    return null
  }
  
  // ✅ Correct - works across realms
  if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
    return null
  }
  throw err
}
```

## Working with GitHub PRs, Issues, and Comments

-   **Open every PR with the repository template.** `gh pr create` does not apply `.github/pull_request_template.md` automatically, so read that file and pass its filled-in contents as the PR body (`--body`/`--body-file`). Keep every section (Summary, Squash Commit Body, Checklist), fill them in for this change, mark the checklist items, and remove only the lines the template says are inapplicable.
-   **Keep PR titles and descriptions current.** When pushing new changes to a PR, review the title and description and update them if they no longer accurately reflect what the PR does.
-   **Reply to and resolve review conversations.** Once a review comment has been addressed, reply to the thread with a description of the resolution including the commit hash that fixed it, then mark the conversation as resolved.
-   **Sign all agent-authored content.** When posting a comment, creating an issue, or opening a PR, append a footer to the message indicating that it was written by an agent. The footer must include the name of the agent and the name of the model used. Example:

    ```markdown
    ---
    Written by an agent (Claude Code, claude-opus-4-7).
    ```

## Resolving Conflicts in GitHub PRs

Use `shell/resolve-pr-conflicts.sh` to resolve PR conflicts:

```bash
./shell/resolve-pr-conflicts.sh <PR_NUMBER>
```

The script force-fetches the base branch (avoiding stale refs), rebases, auto-resolves `pnpm-lock.yaml` conflicts via `pnpm install`, force-pushes, and verifies GitHub sees the PR as mergeable. For non-lockfile conflicts it will pause and list the files that need manual resolution.

## Key Configuration Files

-   `pnpm-workspace.yaml`: Defines the workspace structure.
-   `package.json` (root): Root scripts and devDependencies.
-   `CONTRIBUTING.md`: Detailed contribution guidelines.
