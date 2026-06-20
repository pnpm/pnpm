# Agent Guide to pnpm Repository

This document provides context and instructions for AI agents working on the pnpm codebase.

The repository contains two stacks:

- The **TypeScript pnpm CLI** — everything outside `pacquet/`.
- The **Rust pacquet port** — `pacquet/`. See [`pacquet/AGENTS.md`](./pacquet/AGENTS.md) for pacquet-specific rules; it adds to (and never contradicts) the conventions below.

Sections below marked "(TypeScript only)" do not apply to pacquet. Everything else applies to both stacks.

## Keep pnpm and pacquet in sync

The two stacks are parallel implementations of the same CLI — pacquet is a Rust port of pnpm whose behavior, flags, defaults, error codes, file formats, and lockfile shape are meant to match pnpm exactly. **Any user-visible change has to land in both.**

When you change one side, do the equivalent change on the other in the same PR if you can. If you can't (different expertise, scope too large, or pacquet hasn't ported the surrounding feature yet), open the PR with just your side — call out in the description what still needs porting, and someone else will push the matching commits to the same PR before it lands.

"User-visible" means anything that affects the CLI surface or the on-disk contract: command-line flags and defaults, environment-variable handling, lockfile/manifest/state-file format, error codes and messages, log emissions parsed by `@pnpm/cli.default-reporter`, store layout, hook semantics. Pure internal refactors, perf wins, and TS-only test cleanups don't need mirroring.

**Scope caveat:** pacquet's current surface area is the dependency-management commands — `install`, `add`, `update`, and `remove`. Every other command (`publish`, `exec`, `run`, `dlx`, `audit`, etc.) lives only in the TypeScript code, so changes there don't need a pacquet-side port yet. The parity rule will widen as pacquet ports more commands; check what pacquet exposes before deciding whether your change is in scope.

The pacquet-side obligation — pnpm is the source of truth, pacquet ports from it, never the other way around — is spelled out at [`pacquet/AGENTS.md`](./pacquet/AGENTS.md#the-cardinal-rule).

## Repository Structure

The pnpm codebase is a monorepo managed by pnpm itself. The root contains functional directories organized by domain:

### Core Directories

-   `pnpm/`: The CLI entry point and main package.
-   `pkg-manager/`: Core package management logic (installation, linking, etc.).
-   `resolving/`: Dependency resolution logic (resolvers for npm, tarballs, git, etc.).
-   `fetching/`: Package fetching logic.
-   `store/`: Store management logic (content-addressable storage).
-   `lockfile/`: Lockfile handling, parsing, and utilities.

### CLI & Configuration

-   `cli/`: CLI command implementations and infrastructure.
-   `config/`: Configuration management and parsing.
-   `hooks/`: pnpm hooks (readPackage, etc.).
-   `completion/`: Shell completion support.

### Other Functional Directories

-   `network/`: Network-related utilities (proxy, fetch, auth).
-   `workspace/`: Workspace-related utilities.
-   `exec/`: Execution-related commands (run, exec, dlx).
-   `env/`: Node.js environment management.
-   `cache/`: Cache-related commands and utilities.
-   `patching/`: Package patching functionality.
-   `reviewing/`: License and dependency review tools.
-   `releasing/`: Release and publishing utilities.

### Shared Utilities

-   `packages/`: Shared utility packages (constants, error handling, logger, types, etc.).
-   `fs/`: Filesystem utilities.
-   `crypto/`: Cryptographic utilities.
-   `text/`: Text processing utilities.

### Rust Port

-   `pacquet/`: The pnpm CLI ported to Rust. Self-contained sub-project with its own crates, tests, and tooling — see [`pacquet/AGENTS.md`](./pacquet/AGENTS.md).

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

**Important:** The pnpm CLI e2e tests (in `pnpm/test/`) use the **bundled** `pnpm/dist/pnpm.mjs`, not the individual package `lib/` outputs. After changing any package, you must rebuild the bundle before running e2e tests:

```bash
pnpm --filter pnpm run compile
```

This runs `tsgo --build`, linting, and `pnpm run bundle` (which bundles all packages into `pnpm/dist/pnpm.mjs`). Without this step, e2e tests will use a stale bundle and your changes won't be tested.

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

The repository's review framework lives in **[REVIEW_GUIDE.md](./REVIEW_GUIDE.md)** — how changes are accepted or rejected, the security-first / performance-second priorities, the security checklist and advisory regression themes, and the test/changeset/parity expectations. Apply it when reviewing pull requests. (Code style, comments, and engineering conventions for the TypeScript CLI are documented in the "Code Style" section of this file; pacquet and pnpr follow their own `AGENTS.md` and style guides.)

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

## Changesets (TypeScript only)

If your changes affect published packages, you MUST create a changeset file in the `.changeset` directory. The changeset file should describe the change and specify the packages that are affected with the pending version bump types: patch, minor, or major. Write the description for pnpm users and keep it concise — it becomes a release note. Implementation rationale belongs in the commit message, not the changeset.

**IMPORTANT: Always explicitly include `"pnpm"` in the changeset** with the appropriate version bump (patch, minor, or major). The pnpm CLI will only receive automatic patch bumps from its dependencies, so if your change warrants a minor or major version bump for the CLI, you must specify it explicitly. The changeset description will appear on the release notes page.

Example:

```
---
"@pnpm/installing.deps-installer": minor
"pnpm": minor
---

Added a new setting `blockExoticSubdeps` that prevents the resolution of exotic protocols in transitive dependencies [#10352](https://github.com/pnpm/pnpm/issues/10352).
```

**Versioning Guidelines for pnpm CLI:**
- **patch**: Bug fixes, internal refactors, and changes that don't require documentation updates
- **minor**: New features, settings, or commands that should be documented (anything users should know about)
- **major**: Breaking changes

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

### Comments

Write code that explains itself. A reader should understand what a function does from its name, parameters, and types — not from prose above the call site.

Defaults:

-   **Do not write a comment** that restates what the code already says. If renaming a variable, splitting a helper, or moving a check to a more obvious place would carry the information, do that instead.
-   **Do not repeat documentation** at call sites that already lives on the callee. If the function has a JSDoc, the call site shouldn't re-explain what calling it does. Update the JSDoc once; let every call site benefit.
-   **Put a shared *why* in one place.** When the same rationale underlies several related functions — peers that delegate to a common helper, or a type and its methods — document it once at that common home and reference it from the rest, instead of re-deriving it in each. This is the call-site rule applied sideways across peers, not just upward to a callee.
-   **JSDoc is for the function's contract** — preconditions, postconditions, edge cases, why the function exists. Not for re-narrating the body.
-   **Do not record past implementation shape, refactor history, or "the previous code did X" framing.** That's what `git log` and `git blame` are for. Describe the current contract — what the code is and what it guarantees — not what it replaced. Phrasings like "used to", "previously", "the original X", or a parenthetical naming a removed type belong in the commit message, not in the source.

Write a comment only when:

-   The reason for the code is non-obvious from reading it (a hidden invariant, a workaround for a known bug, a deliberate exception to the surrounding pattern).
-   The right name doesn't fit — e.g., a temporary technical constraint that's worth flagging but doesn't justify a new symbol.

Before adding a comment, ask: "Could I rename, restructure, or extract instead?" If yes, do that. The bar for prose-in-code is high; the bar for prose-that-restates-code is "don't."

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
