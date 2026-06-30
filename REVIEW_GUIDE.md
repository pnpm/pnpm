# pnpm Code Review Guide

How changes to `pnpm/pnpm` are reviewed, and how the decision to accept, reject, narrow, or
redesign a change is made — a framework a human or AI reviewer can apply to reach the same bar.

The central question for any PR:

> Does this change solve a real pnpm problem, in the right layer, with a contained
> user-visible contract, and without unacceptable security, performance, compatibility, or
> maintenance cost — and is it the *smallest correct version* of itself?

This is the canonical review guide for the repository, applied by human reviewers and by two
automated reviewers — CodeRabbit ([`.coderabbit.yaml`](./.coderabbit.yaml)) and Qodo
([`.pr_agent.toml`](./.pr_agent.toml)). To cut duplicate comments the bots divide review depth —
CodeRabbit leads correctness and conventions, Qodo leads security and performance — but every
reviewer applies the same priority order below. [`AGENTS.md`](./AGENTS.md) points here.

## Review priorities

1. **Security first** (§1).
2. **Performance second** (§2) — `install`, `add`, `update`, `remove`, resolution, fetching,
   linking, lockfile handling, store access, and pnpr request paths are hot paths.
3. **Product fit** — add surface (commands, settings, output) only when the benefit is clear
   and the result is maintainable.
4. **Maintainability always** — prefer the right abstraction and package boundary over a
   one-off patch.

---

## 1. Security review rules

Security is the first priority — review with a security-first lens. Treat manifests, lockfiles,
registry metadata, tarballs, paths, environment variables, lifecycle scripts, workspace config,
and git metadata as attacker-controlled. Surface plausible issues even when they're edge cases,
but always explain the exploit path and impact on the *changed* code; never give generic
security advice untethered from the diff. Security fixes themselves need precise threat modeling:

- What input is attacker-controlled? Can a repo, package, registry response, lockfile, tarball,
  env var, or path influence a **trust decision**?
- Does the bug expose end users, or only dev dependencies/tests in the repo?
- Is the fix in the right layer, or does it only patch one call site?
- Does it keep pnpm/pacquet parity where behavior is shared?

Treat as attacker-controlled: package metadata, tarball contents, lockfiles, workspace
manifests, `.npmrc`/environment config, registry responses, git URLs, filesystem paths, and
script names.

Look especially for:

- unsafe handling of package manifests, lockfiles, tarballs, registry responses, lifecycle
  scripts, `configDependencies`, patches, and workspace links;
- path traversal, symlink/hardlink, archive extraction, arbitrary file read/write/delete,
  TOCTOU, and permissions mistakes;
- command injection, shell-argument construction, environment-variable trust, executable
  resolution, script-execution policy, and privilege-boundary mistakes;
- registry/network/auth mistakes: token leakage, proxy handling, redirect behavior, TLS
  assumptions, cache poisoning, integrity/hash verification, and downgrade/confusion attacks;
- Rust memory, concurrency, and unsafe-FFI issues in pacquet, including panic-on-untrusted-input
  denial of service.

Advisory regression themes — recurring classes from past pnpm advisories:

- repo-controlled `.npmrc` and `pnpm-workspace.yaml` must not expand victim environment secrets
  into registry URLs, auth headers, proxy settings, token helpers, or other outbound requests;
- user-level npm auth credentials must not be bound to a repository-selected registry unless the
  registry scope and trust boundary are explicit;
- lifecycle/build-script approval gates must cover all dependency sources and phases — git
  dependencies, fetch/prepare/prepack/prepublish paths, `allowBuilds`, ignored-build reporting,
  explicit denials, and pacquet parity;
- opaque dependency identities (git, URL, tarball, file, directory, patch, alias locators) stay
  byte-for-byte exact where used for trust; don't normalize away attacker-controlled suffixes or
  confuse them with registry peer suffixes;
- lockfile entries for remote/dynamic deps, GitHub/git deps, tarballs, commits, and integrity
  fields must preserve enough immutable integrity data to reject changed content and avoid
  missing-field bypasses;
- treat lockfile fields and git metadata as untrusted input (especially `resolution.commit`,
  refs, URLs); prevent command/argument injection and never pass attacker-controlled values as
  executable flags;
- path handling must reject traversal and root escape in bin names, `directories.bin`,
  transitive aliases, patch files, tar/zip entries, symlink/hardlink targets, file/git deps,
  Windows path separators, executable shims, permission changes, and delete/write destinations;
- cache/store/global-metadata keys must include all trust-relevant inputs so overrides, scripts
  policy, registry metadata, and lockfile state can't poison later installs or other workspaces;
- archive extraction and package-identity code must match npm/registry semantics for duplicate
  tar entries, stripped path components, symlinks, permissions, and manifest selection;
- path-shortening, hashing, cache-naming, and content-addressing code must use
  collision-resistant identifiers, and verify collisions can't redirect deps or overwrite
  package contents.

Recurring judgement calls:

- **Never strip or normalize an attacker-relevant identifier.** Opaque/locator identities (git,
  URL, tarball, jsr/gh prefixes, registry suffixes) stay byte-for-byte exact where used for
  trust — stripping a suffix can bind a name to a package you don't own.
- **Atomic, fail-safe file writes.** `wx`/exclusive-create is not atomic — a crash mid-write
  leaves an invalid file in the store. Use temp-file-plus-rename, or write `package.json` last
  as a completion marker.
- **Repo-controlled `.npmrc`/workspace config** must not expand victim secrets into outbound
  registry/auth/proxy values. Token helpers are executable code — only from trusted/user
  config. Suggested copy-paste commands must not embed shell-expandable attacker-controlled keys.
- **CI/workflow changes** handling fork-influenced artifacts use minimum token permissions and
  scope secrets to the exact step that needs them.
- **Lifecycle/build-script trust gates** cover every dependency source and phase.

**Don't overreact to audit output.** An advisory on dev-only/non-runtime code, or on functions
pnpm doesn't call, may be safe to ignore; don't force a semver-breaking update to silence a
non-exploitable warning. A misconfigured consumer is not a pnpm bug.

**Security defaults still need a performance design.** A gate that adds a registry request per
lockfile package needs a cache/fast path (e.g. a per-lockfile cache keyed on the lockfile hash)
before it ships as a default.

---

## 2. Performance review rules

pnpm is performance-sensitive; be skeptical of extra work in common flows.

- **Never trade correctness for a micro-optimization.** This is pacquet's cardinal rule — match
  pnpm exactly even when that's slower. Declining an optimization that risks order-dependent
  output, breaks an invariant, or couples concerns is correct.
- **A performance change must be measured.** If it's pitched as perf, there must be a number.

Reject or redesign changes that add thousands of filesystem ops per install, a network round
trip per dependency, full metadata fetches where smaller/cached data would do, repeated
parsing/scans in resolver/linker loops, noisy logs or expensive checks on the warm path, or
broad fast-path invalidation for rare cases. Prefer cheap markers and existing invariants.

Benchmarks should use multiple runs (with variance when the gap is small); hot/warm/cold store
and cold-install rows where relevant; realistic dependency counts (2000–3000 packages can be a
small project); and evidence the win is on a common path. A negligible gain that adds
complexity isn't worth it — and the same evidence justifies *refusing* an optimization (cost is
sub-0.1% or off the hot path).

---

## 3. First-pass triage: should this exist at all?

Many PRs are closed not because they're buggy but because the change doesn't make sense for
pnpm. Before line-by-line review, ask:

- **Is it duplicated or already solved?** By another PR, an existing command or setting.
- **Is it the right behavior?** Semantics come first.
- **Does it actually help?** A performance PR with no measurement, or a negligible measured
  gain, gets closed (see §2).
- **Is the trade-off worth it?** A cosmetic or micro win that adds complexity, coupling, or
  risk is a net negative.

The single most useful question is **"What is the benefit of this?"** When you reject, say why
in a sentence or two and link the superseding PR if one exists.

### When a change doesn't belong

Push back on changes that are mostly churn:

- Style/grammar edits that don't change behavior.
- Refactors that don't reduce complexity or unlock a needed change.
- Commands or aliases added just because another tool has them.
- Rare-case handling that adds permanent CLI complexity.
- Error-prone "nice to have" compatibility that duplicates a better pnpm mechanism.

Judge a change on the change itself; how it was authored is not a review criterion.

---

## 4. Scope discipline

One PR does one thing.

- Unrelated changes get pulled out.
- Dangerous or sweeping optimizations must be split so each can be reasoned about separately.
- One changeset per logical change; don't bundle unrelated changes.

For every touched file, ask "why was this needed for *this* PR?" If it isn't obvious from the
goal, it comes out.

---

## 5. Product and UX rules

### Use established command semantics

When matching npm functionality, use npm's recognized command name and behavior unless there's
a deliberate reason to differ (`npm view` → `pnpm view`, not a new name or alias). Support the
spec forms users expect (`foo@2`, dist-tags) without duplicating resolver logic. Don't copy
npm's command sprawl.

### Interop features must actually match

Prior art from npm, Yarn, vlt, etc. is useful but not automatic justification. Check that
syntax and semantics genuinely match pnpm. Steer users to the right existing mechanism instead.

### Defaults and contracts are hard to change

Changing a default usually needs a major version, a strong user signal (a poll or widespread
demand), or an opt-in setting first.

### Avoid user-visible noise

Don't add large or noisy logs unless the user can act on them.

---

## 6. Architecture rules

### Put logic in the owning layer

- Spec parsing belongs in resolvers, not scattered callers.
- Git/tarball policy checks belong in the git/tarball resolvers, returning verifiers that
  validate resolutions against active policies — the same way the npm resolver verifies
  `minimumReleaseAge` and `trustPolicy`.
- Workspace selection belongs in the CLI dispatch layer, not re-derived inside handlers with
  partial options.
- If a runtime path needs metadata that resolution already computed, pass it through rather
  than re-fetching or re-parsing.

Avoid fixes that make unrelated packages learn resolver-specific details; prefer a precise gate
where the ambiguous value is consumed.

### Reuse before you write — but don't add ceremony

Duplication is a frequent problem in this monorepo. Search `packages/`, `fs/`, `crypto/`,
`text/`, `default-reporter`, and the manifest/lockfile utilities first, and prefer a maintained
package over a custom reimplementation of parsing, serialization, or shell escaping. But reuse
is the goal, not abstraction for its own sake — don't extract an indirection that doesn't
reduce coupling.

---

## 7. Test expectations

Tests must prove the changed behavior, not just execute nearby code.

- A **regression test that fails without the fix**.
- The **right level**: e2e through the real CLI for wiring/filters/recursive/config/output;
  unit tests for narrow parsing, validation, and resolver decisions. Don't write a heavy e2e
  test for a simple pure function.
- **Meaningful, not vacuous**: assert the effect actually happened, so the test can't pass on
  an empty array or unchanged fixture.
- **Cross-platform where it matters**: Windows and concurrent-write coverage for path, symlink,
  hardlink, and locking behavior.
- **Conventions**: tests in a separate file; no hardcoded checksums (use registry-mock's
  `getIntegrity()`); don't depend on unreleased Node.js behavior.
- Never dismiss a failure as "pre-existing" — investigate and fix it in the PR.

---

## 8. Changesets, docs, and versioning

- **User-visible change to a published package → changeset required.**
- **Test-only or internal change (not visible to CLI users) → none.**
- **Behavior/setting users should know about → changeset and usually docs.**
- **Always include `"pnpm"` explicitly** with the right bump: `patch` (bug fix / internal),
  `minor` (feature / setting), `major` (breaking).
- **One changeset per logical change.** The text is a user-facing release note — accurate and
  concise, no implementation rationale — and it must match what the code actually does.
- **pacquet-only PRs don't get changesets.**

---

## 9. pnpm ↔ pacquet parity

pnpm is the source of truth; pacquet is the Rust port that matches it exactly. **Every command
should have a pacquet equivalent, and every user-visible change must land in both products** —
flags, defaults, env handling, error codes/messages, lockfile shape, store layout, build
policy, lifecycle behavior, config handling, output. A change without its pacquet counterpart
is incomplete.

- Do both sides in one PR when practical; otherwise open with one side and state in the
  description what still needs porting.
- Don't make pacquet "better" than pnpm independently — land the behavior in pnpm first, then
  mirror.
- When a bot says a symbol is "not referenced," it may be searching only one major's branch;
  check the other branch.

---

## 10. Dependencies and external packages

Before adding a dependency or writing custom logic: check for an existing repo utility; compare
maintained packages that already solve it; add it at the narrowest package that needs it; avoid
heavy dependencies for small conveniences; and don't hand-roll serialization, parsing, or shell
escaping when a proven library or local helper exists.

---

## 11. PR hygiene

- One logical change per PR.
- Don't open a duplicate PR for the same change; update the existing one.
- Let maintainers push when collaboration is faster.
- Rebase when the branch goes stale.
- Use the PR template and keep the title/summary current.
- Resolve a review thread only after the issue is fixed (link the fixing commit) or explicitly
  declined with rationale.
- Bot reviews (CodeRabbit/Qodo/Copilot) are usually correct, and a human typically reviews only
  after the bots approve. Read each suggestion and classify it (valid / false positive / already
  fixed / out of scope) rather than dismissing or blindly applying it.

---

## 12. How feedback is written

The voice is short, direct, and specific.

- **Ask "why," don't just assert.** A question that exposes an unjustified change beats a
  paragraph.
- **Use GitHub `suggestion` blocks** for exact wording.
- **Be honest about uncertainty** — and name the scenario that worries you.
- **When you decline a suggestion, justify it concretely** — the invariant it would break, the
  cost it would add, or the measurement that makes it moot.
- **When you accept feedback, link the fixing commit** and reply on the thread before resolving.
- Don't nitpick what linting already covers.

---

## 13. Reviewer's checklist

For each PR, in order:

1. **Should it exist?** Real, in scope, not duplicated, worth the cost/risk. (§3)
2. **Security.** Walk the §1 checklist against the diff; explain any exploit path. (§1)
3. **Performance.** Hot path or pitched as perf? Evidence at realistic scale? (§2)
4. **Scope.** Every touched file justified; unrelated/dangerous changes split out. (§4)
5. **Layer & reuse.** Logic in the owning layer; no reimplementation; no needless abstraction. (§6)
6. **Product/contract.** npm-recognized semantics; defaults preserved or properly gated;
   no log noise. (§5)
7. **Tests.** Right level, meaningful, regression-proving, cross-platform where relevant. (§7)
8. **Changeset.** Present iff user-visible; `"pnpm"` included; one per change; accurate. (§8)
9. **Parity.** pacquet equivalent handled or explicitly deferred. (§9)
10. **Conventions.** `PnpmError`, no swallowed errors, good names, reused libraries, correct
    dependency placement, config through options. (`AGENTS.md` → Conventions)

A change is mergeable when it is the **smallest correct, secure, in-scope version of a thing
pnpm should do**, in the right layer, proven by a meaningful test, documented if user-visible,
and mirrored in pacquet.
