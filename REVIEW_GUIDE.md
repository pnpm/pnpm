# pnpm Code Review Guide

How changes get reviewed in `pnpm/pnpm`, and how the decision to accept, reject, narrow,
or redesign a change is actually made — in the style used by Zoltan Kochan. It distills the
recurring patterns from ~3,600 of the maintainer's review comments spanning 2016–2026 (PR
threads, line-level reviews, and PR-closing decisions) into a repeatable framework, so a
human reviewer *or* an AI reviewer can apply the same bar. Representative threads are linked
at the end and inline where useful.

The central question for any PR:

> Does this change solve a real pnpm problem, in the right layer, with a contained
> user-visible contract, and without unacceptable security, performance, compatibility, or
> maintenance cost — and is it the *smallest correct version* of itself?

This complements the **"AI Review Guidance"** section of [`AGENTS.md`](./AGENTS.md), which
defines the security-first / performance-second priority order that the bots
(`.coderabbit.yaml`, `.pr_agent.toml`) enforce. This guide is the human judgement layered on
top.

## Review priorities

1. **Security first.** Treat manifests, lockfiles, registry metadata, tarballs, paths,
   environment variables, lifecycle scripts, workspace config, and git metadata as
   attacker-controlled.
2. **Performance second.** `install`, `add`, `update`, `remove`, resolution, fetching,
   linking, lockfile handling, store access, and pnpr request paths are hot paths.
3. **Product fit next.** pnpm stays focused. Don't add command surface, settings, log noise,
   or compatibility layers unless the benefit is clear.
4. **Maintainability always.** Prefer the right abstraction and package boundary over a
   one-off patch.

---

## 1. First-pass triage: should this exist at all?

A large share of PRs are closed not because they're buggy but because the change doesn't
make sense for pnpm. Ask these before any line-by-line review:

- **Is the problem real, current, and relevant to pnpm users?** Re-check `main` first —
  *"There are no audit errors currently on main branch."* Many closes are *"superseded by
  #NNNN"*, *"related code was removed"*, *"redundant and based on a stale `main`."*
- **Is it duplicated or better solved elsewhere?** By another PR, an existing command, a
  setting, `pnpm patch`, `pnpm fetch`, catalogs, a docs change, or a separate tool.
- **Is it the right behavior?** Semantics come first: *"this is not correct behavior for
  `pnpm install --dry-run`"* ([#12270](https://github.com/pnpm/pnpm/pull/12270#issuecomment-4717095101)).
- **Is it in scope?** A feature request is not a bug: *"Config dep removal is out of scope
  for this PR — this is a feature request, not a bug in the current code."*
- **Is it on the right branch?** A v10 fix shouldn't target `main` if `main` is v11-only.
- **Does it help?** Perf PRs with no measurement, or with negligible measured gain, are
  closed: *"this didn't help performance."*, *"It doesn't make a difference."* (see §6).
- **Is the trade-off worth it?** *"the cost/risk isn't justified for a manifest-cosmetic
  change."* A cosmetic or micro win that adds complexity, coupling, or risk is a net negative.

A common, sufficient review question is simply: **"What is the benefit of this?"**

When you reject, **say why in one or two sentences and link the superseding PR/commit** if
one exists. Rejections are short, direct, and specific.

### When a change doesn't belong

Push back on changes that are mostly churn:

- Pure grammar/style edits that don't improve behavior or maintenance.
- Refactors that don't reduce real complexity or unlock a necessary change —
  *"was it really needed to rewrite the whole function?"*
- Extra CLI commands or aliases added just because npm or another tool has them.
- Rare scenarios that would add permanent complexity to the CLI.
- "Nice to have" compatibility that is error-prone or duplicates a better pnpm mechanism.

Judge a change on the change itself — whether it fits how pnpm works, says something the code
doesn't already, and has a reason to exist. Provenance is irrelevant: how a PR was authored
is not a review criterion.

### Know the domain invariants before judging a change

Many rejections come straight from package-manager domain knowledge. Hold these as fixed
points and check a change against them:

- **Peer dependencies are singletons.** A project should not install two versions of a peer;
  a peer range is only a suggestion, and pnpm reuses an already-installed version. Fixes that
  imply multiple peer versions, aliased peers, or exact-pinned peers are usually wrong —
  *"peer dependencies should be singletons"*, *"I don't think aliases make sense in peer deps."*
- **`package.json` is the only manifest that matters.** Don't add support for more manifest
  formats; Node.js reads `type`/config only from `package.json`.
- **Workspace projects share `<root>/node_modules/.pnpm`** — they don't each own a store; this
  drives where patches, hoisting, and links can live.
- **Don't reason about pnpm as if it were npm.** It isn't always installed from npm, runs its
  own bundled Node only to run itself, and the project now ships its own registry (pnpr) — so
  registry-side behavior is in scope where pnpr is involved, not automatically "someone else's
  problem."

---

## 2. Scope discipline

One PR does one thing. This is enforced aggressively.

- Unrelated changes get pulled out: *"this is a totally unrelated change. I don't understand
  why it was added to this pr"*, *"why was a change in this file needed?"*, *"how is this
  related at all to the `pnpm bugs` command?"*
- Dangerous or sweeping optimizations must be split so each can be reasoned about
  separately: *"There are a lot of dangerous changes in the PR. Can you create a separate PR
  for every optimization? We'll have to think it through very carefully."*
  ([#11083](https://github.com/pnpm/pnpm/pull/11083#issuecomment-4120809436))
- Changesets follow scope: *"don't create one changeset for two unrelated changes. Create
  separate changeset files."*

**Reviewer reflex:** for every touched file, ask "why was this needed for *this* PR?" If the
answer isn't obvious from the PR's stated goal, ask — or have it removed.

---

## 3. Product and UX rules

### Stay inside pnpm's mission

pnpm is a **package manager**, not a task runner or build orchestrator. Complex task
configuration, scheduling, and the like are out of scope — *"We concentrate on package
management not running tasks … maybe you should use a dedicated tool."* When a request is
better solved in the user's own deps or by a different tool, say so and point there rather
than growing pnpm's surface.

### Don't add strictness that trips legitimate workflows

Before adding a warning, error, or "clean up unused X" enforcement, imagine the legitimate
workflows it would punish — shared org-wide config that intentionally carries entries not used
in every repo is a recurring example. When some signal is still useful, prefer an **opt-in
command** over a warning everyone pays for. False positives on valid setups are worse than
silence.

### Use established command semantics

When matching npm functionality, use npm's recognized command name and behavior unless pnpm
has a deliberate reason to differ. Implementing `npm view` → the command is `pnpm view`, not
a new name or extra alias ([#11064](https://github.com/pnpm/pnpm/pull/11064#issuecomment-4107201643)).
Support the spec forms users expect (`foo@2`, dist-tags) without duplicating resolver logic.
Don't copy npm's command sprawl — a command belongs in pnpm only if it's worth maintaining.

### Interop features must actually match

Prior art from npm, Yarn, vlt, etc. is useful but not automatic justification. Check that
syntax and semantics genuinely match pnpm. Re-adding Yarn `resolutions` was rejected because
the syntax doesn't match pnpm `overrides` and would be error-prone — a deprecation warning
was enough ([#11941](https://github.com/pnpm/pnpm/pull/11941#issuecomment-4740765822)).
Steer users to the right existing mechanism instead (catalogs over the `$` override syntax;
the global virtual store over filtered installs).

### Defaults and contracts are hard to change

A change to a default usually needs one of: a major version; a strong user signal (an
explicit poll or widespread demand); or an opt-in setting first.

- **Dry-run is a strict contract:** it behaves like the real command except it must not
  change `node_modules`, lockfiles, manifests, or other project state. A setting that would
  alter that "will break all commands that work with dry-run"
  ([#12080](https://github.com/pnpm/pnpm/pull/12080#issuecomment-4726690874)).
- **Settings live where users expect them.** Workspace-wide policy belongs in
  `pnpm-workspace.yaml` (camelCase), with the kebab-case CLI flag shown alongside. pnpm only
  reads auth/network keys from `.npmrc`: *"we don't read settings from here anymore."*,
  *"v11 read settings from the global rc file of pnpm, not from `.npmrc`."*
- A behavior-changing strict setting **must be prominently documented** on the website.
- **Don't bump versions for under-development features:** *"this is still under development.
  no need to bump anything."*

### Avoid user-visible noise

Don't add large or noisy logs unless the user can act on them. Printing every skipped
version, transitive detail, or irrelevant advisory in install/update flows can be worse than
saying nothing.

---

## 4. Architecture rules

### Put logic in the owning layer

Review whether the change solves the problem at the right boundary:

- Spec parsing belongs in resolvers, not scattered callers.
- Git and tarball policy checks belong in the git/tarball resolvers, returning
  `ResolutionVerifiers` that validate resolutions against active policies — *"This needs a
  bigger rewrite. The checks should happen inside the git-resolver and tarball-resolver …
  similarly how npm resolver verified minimumReleaseAge and trustPolicy."*
  ([#11805](https://github.com/pnpm/pnpm/pull/11805#issuecomment-4508128892))
- Workspace selection belongs in the CLI dispatch layer, not re-derived inside command
  handlers with incomplete options.
- If a runtime path needs metadata already computed by resolution, pass it through rather
  than re-fetching or re-parsing.

Avoid fixes that force unrelated packages to know resolver-specific details. If a proposed
hardening adds cross-resolver knowledge, look for a more precise gate where the ambiguous
value is consumed.

### Reuse before you write — but don't add ceremony

Duplication is a real, frequently-flagged problem in this monorepo. Search `packages/`,
`fs/`, `crypto/`, `text/`, `default-reporter`, and the manifest/lockfile utilities first.

- *"we already have libraries for reading the package manifest"*
- *"I think we already have similar code in the default-reporter. Maybe deduplicate it."*
- Prefer a maintained open-source package over a custom reimplementation of parsing,
  serialization, or shell escaping.

Reuse is the goal, **not** abstraction for its own sake: *"Not worth extracting a constant
for this — it's only written in one place … A constant would add indirection without
reducing coupling."* Conversely, when a PR adds *"so much logic change"* to an existing
module, a new class/helper with a narrower responsibility may be the safer shape.

### Treat lockfile-format changes as high risk

The lockfile format is the product of many iterations and has many users. A format change
must justify: why the existing format can't support the use case; how conflicts, diffs, and
install auto-resolution are affected; whether the same conflict just moves to more files;
whether extra filesystem ops are worth it; and whether old lockfiles stay readable and
stable. Prefer minimal extensions that preserve the existing compact forms.

---

## 5. Security review rules

Apply the full **"AI Review Guidance"** checklist in `AGENTS.md`. Security fixes are welcome
but require precise threat modeling. Ask:

- What input is attacker-controlled? Can a repo, package, registry response, lockfile,
  tarball, env var, or path influence a **trust decision**?
- Does the bug expose end users, or only dev dependencies/tests in the repo?
- Is the fix in the right layer, or does it only patch one call site?
- Does it preserve pnpm/pacquet parity where behavior is shared?

Recurring judgement calls:

- **Never strip or normalize away an attacker-relevant identifier.** *"You are actually
  introducing a security vulnerability by stripping it as you might not own the package in
  the npm registry. It should not be stripped."* Opaque/locator identities (git, URL,
  tarball, jsr/gh prefixes, registry suffixes) stay byte-for-byte exact where used for trust.
- **Atomic, fail-safe file writes.** Reject `wx`/exclusive-create as an "atomic" claim:
  *"`wx` just means it won't overwrite an existing file. If the process fails mid-write, an
  invalid file will be saved into the store. That never happens with the current rename
  approach."* Temp-file-plus-rename (or writing `package.json` last as a completion marker)
  is the bar for store writes.
- **Repo-controlled `.npmrc`/workspace config** must not expand victim secrets into outbound
  registry/auth/proxy values. Token helpers are executable code — only from trusted/user
  config. Suggested copy-paste commands must not embed shell-expandable attacker-controlled keys.
- **CI/workflow changes** that process fork-influenced artifacts use the minimum token
  permissions and keep secrets scoped to the exact step that needs them.
- **Lifecycle/build-script trust gates** cover every dependency source and phase. Don't run
  postinstall where the environment may not support it.

**Don't overreact to audit output.** If an advisory applies only to dev-only code,
non-runtime paths, or versions/functions pnpm doesn't call, ignoring it may be correct
([#11445](https://github.com/pnpm/pnpm/pull/11445)). Don't force semver-breaking dependency
updates to silence a non-exploitable warning. And a misconfigured consumer is not a pnpm bug:
*"Regardless of this change, your CI is clearly misconfigured if your secrets can [be
exfiltrated]"* ([#11583](https://github.com/pnpm/pnpm/pull/11583#issuecomment-4463511857)).

**Security defaults still need performance design.** A gate that adds a registry request per
lockfile package must ship with a cache/fast path before it becomes default — e.g. a global
per-lockfile cache keyed on the lockfile hash ([#11583](https://github.com/pnpm/pnpm/pull/11583#issuecomment-4463511857)).

---

## 6. Performance review rules

pnpm is performance-sensitive by default; be skeptical of extra work in common flows.
Two rules that look opposed but aren't:

1. **Never trade correctness for a micro-optimization.** For pacquet this is the cardinal
   rule (*"correctness over speed per the cardinal rule"*) — it ports pnpm exactly, even when
   that means rejecting cache hits and running slower. Declining a suggested optimization
   because it risks order-dependent output, breaks an invariant, or couples concerns is the
   right call: *"The cost is immaterial … dwarfed by the install's network/disk/linking work."*
2. **A performance change must be measured.** If it's pitched as perf, there must be a number.

Reject or redesign changes that add: thousands of filesystem ops during install; new network
round trips per dependency; extra full metadata fetches where cached/smaller data would do;
repeated parsing/scans in resolver/linker loops; noisy logs or expensive checks on the warm
path; or broad invalidation of fast paths for rare cases. Prefer cheap markers and existing
invariants — *"package.json written last is already the completion marker"* makes an extra
import-state check redundant ([#11170](https://github.com/pnpm/pnpm/pull/11170#issuecomment-4181025884)).

**Benchmark methodology** — ask for, and provide:

- multiple runs, not a single timing, with variance analysis when the difference is small;
- hot-cache/hot-store, warm-store, cold-store, and cold-install rows where relevant;
- realistic dependency counts — **2000–3000 packages can be a *small* project**
  ([#11583](https://github.com/pnpm/pnpm/pull/11583#issuecomment-4462989559));
- evidence the win is on a common path, not a rare local branch-switching case.

A negligible measured gain that adds complexity is closed with the table attached
(e.g. dependency-range compression showing ~0% gzipped savings). A *refusal* to optimize is
justified the same way — by showing the cost is sub-0.1% or off the hot path ("called at most
twice per install, not per request").

---

## 7. Test expectations

Tests must prove the changed behavior, not just execute nearby code.

- **Regression test that fails without the fix.** Best replies cite it: *"Added a regression
  test (`with-peer-workspace-link`) … it fails on the old code."*
- **Right level.** e2e through the real CLI when wiring/filters/recursive/config/output
  change; unit tests for narrow parsing, validation, resolver decisions. *"I think it is a
  too heavy test for such a simple scenario. Maybe just use a unit test instead."*
- **Meaningful, not vacuous.** *"This test doesn't make any sense in my opinion."* Assert the
  work actually happened so a test can't pass on an empty array or unchanged fixture.
- **Cross-platform where it matters.** Require Windows/concurrent-write coverage for path,
  symlink, hardlink, locking, and concurrent-write behavior: *"Can we have a test that
  verifies this? Several processes writing the same file at the same time. I am especially
  interested whether this works on Windows."* ([#11087](https://github.com/pnpm/pnpm/pull/11087#issuecomment-4121103840))
- **Conventions:** tests in a separate file; no hardcoded checksums (use registry-mock's
  `getIntegrity()`); don't depend on unreleased Node.js behavior unless the PR is explicitly
  waiting on it.
- Never dismiss a failure as "pre-existing." Investigate and fix it in the PR.

---

## 8. Changesets, docs, and versioning

- **User-visible change to a published package → changeset required.**
- **Test-only change → none:** *"test only changes don't need release notes."*
  ([discussion](https://github.com/pnpm/pnpm/pull/12398#discussion_r3409452082))
- **Internal, not visible to CLI users → none:** *"This is not a change that will be visible
  to pnpm cli users. Hence, no need for a changeset."*
  ([discussion](https://github.com/pnpm/pnpm/pull/12441#discussion_r3435784965))
- **Behavior/setting users should know about → changeset and usually docs.**
- **Breaking change or default change → major**, unless there's an explicit compatibility story.
- **Always include `"pnpm"` explicitly** with the right bump. Features/settings = `minor`;
  bug fixes and internal refactors = `patch`; breaking = `major`.
- **One changeset per logical change.** The text is a user-facing release note — accurate,
  concise, no implementation rationale (that goes in the commit message). It must match what
  the code actually does: don't describe behavior, platforms, or file types the change doesn't
  cover.
- **pacquet-only PRs don't get changesets**, despite the general rule.

---

## 9. pnpm ↔ pacquet parity

pnpm is the source of truth; pacquet is the Rust port that matches it exactly. **Any
user-visible change to dependency-management behavior must land on both sides** — flags,
defaults, env handling, error codes/messages, lockfile shape, store layout, build policy,
lifecycle behavior, config handling, output — for `install`, `add`, `update`, `remove`.

- Do both sides in one PR when practical; otherwise open with one side and state in the
  description what still needs porting.
- Other commands (publish, exec, run, dlx, audit, …) don't need a pacquet port yet.
- **Don't make pacquet "better" than pnpm independently.** If the correct fix changes pnpm
  behavior, land it in pnpm first, then mirror.
- When a bot flags a symbol as "not referenced," it may be searching only one major's branch
  — check the other branch before agreeing.

---

## 10. Dependencies and external packages

Before adding a dependency or writing custom logic: check for an existing repo utility;
compare maintained packages that already solve it; add at the narrowest package level; avoid
heavy dependencies for small CLI conveniences; avoid custom serialization/parsing/shell
escaping when a proven library or local helper exists. If a PR introduces a new prompt,
parser, cache, or datastore dependency, ask how it compares to known alternatives and what it
costs in install size, maintenance, and runtime.

---

## 11. PR hygiene

- One logical change per PR.
- Don't open a duplicate PR for the same change; update the existing one.
- Allow maintainers to push when collaboration is faster.
- Rebase when conflicts or upstream changes make the branch stale.
- Use the PR template and keep the title/summary current.
- Resolve review conversations only after the issue is fixed (link the fixing commit) or
  explicitly declined with rationale.
- Bot comments (CodeRabbit/Copilot/Qodo) are useful but not gospel — classify each as valid,
  false positive, already fixed, or out of scope. Don't apply them blindly.

---

## 12. Engineering conventions (line-level review)

Above the macro accept/reject decision sits a dense layer of code-craft conventions enforced
comment-by-comment. These are the most frequent line-level review notes:

**Errors**
- **User-reachable errors are `PnpmError`** from `@pnpm/error` — they're part of the UX, so
  they carry a stable code — *"Use PnpmError", "You should throw PnpmError."* Errors that can
  only come from programmer mistakes, type guards, or unreachable branches stay plain `Error`;
  don't dress those up with a code.
- **Never swallow errors.** *"don't mute this error"*, *"why should this error be ignored?"*
  Catch only the specific expected code — *"you now ignore any error, not just ENOENT."*
- **Throw on impossible states** rather than continuing: *"This should never happen, so an
  error should be probably thrown."*
- **Error messages must carry context** to be actionable: *"This error is not useful without
  the path to the manifest."* Prefer throwing an error over adding an interactive prompt.

**Naming**
- Functions are verbs (`findDependencyLicenses`, not `licenses`); types and fields are
  specific, not generic (*"The type names are too generic"*, *"This name doesn't make
  sense"*). Reuse existing terminology (`author`, `homepage`) rather than inventing synonyms.
- File names use camelCase / the existing convention; a renamed concept gets renamed
  everywhere it appears.

**Revert unrelated churn**
- *"revert the unrelated formatting change"*, *"avoid change to formatting unrelated to the
  changes"*, *"this change is unrelated, revert it"*, *"this file wasn't modified."* Zero
  tolerance for diff noise — it's the line-level form of §2.

**Reuse repo libraries — don't add a dependency that already exists**
- Concrete recurring picks: `symlink-dir` for symlinks, `micromatch`/`fast-glob` for globs,
  `sort-keys` for sorting, `js-yaml` for YAML, `delay` for timers, `PatchFile` from
  `lockfile.fs`. *"we have a library for creating symlinks"*, *"use micromatch instead, as we
  already have it in deps"*, *"no need to add a new dependency."* Don't pull in a second
  library for a job an existing one does (e.g. two YAML parsers).
- Deduplicate copy-pasted logic into a shared function/package rather than repeating it.

**String parsing — reach for regex last**
- Prefer plain string operations (`split`, `slice`, `indexOf`, `startsWith`, …) over a custom
  regular expression; people and AIs over-reach for clever regex.
- When the input genuinely needs structured parsing with backtracking, use the existing
  **parser-combinator** pattern (e.g. `object/property-path/`, introduced in
  [#9811](https://github.com/pnpm/pnpm/pull/9811)) rather than a god-complex regex.

**Dependency placement**
- Shared infrastructure (the logger, etc.) must be a **peer dependency**, not a regular one —
  *"logger must be a peer dependency."* Put a dep at the narrowest package that needs it; move
  a misplaced dep to `devDependencies` when it isn't a runtime dep.

**Config and layering**
- Configurable values flow through `@pnpm/config` and into commands via options — **don't
  hardcode** them: *"virtualStoreDir is configurable via a setting, so it should be passed in
  via options"*, *"Don't hardcode these retry settings. Get them from the settings."* CLI
  options are camelCased automatically; don't re-map them by hand.
- **Command handlers return data; the CLI prints it.** *"just return the string. The CLI will
  print it. And it is easy to cover with a test."* This keeps handlers unit-testable.
- Don't add a wrapper function with no benefit — *"why do you need this function at all? just
  run X"*, *"I don't see any benefit from using this function. Just create a new one."*

**Async and loops**
- Prefer async fs and `async/await`; run independent work concurrently with `Promise.all` /
  `Promise.any` and `await` things that must complete. Hoist invariant work out of loops —
  *"don't reinitialize the matcher on every iteration. Init it once outside the loop."*

**Comments and style**
- Follow StandardJS: no semicolons, no Prettier. *"we don't use semicolons"*, *"turn off
  prettier … we use standardjs style."*
- Comments must earn their place: *"what is the meaning of this comment?"*, *"how are these
  empty comments useful?"*, *"remove the comments or add the code."* (See `AGENTS.md` → Comments.)

**Tests (line level, complementing §7)**
- *"this needs to be covered with a test"*; no vacuous tests — *"What do these tests test?
  There's nothing happening in the constructor, why test it?"*; assert the actual effect
  (*"you're not testing that the warning is reported — spy on the reporter"*); use `jest.fn`,
  not sinon; put the test in the owning package and a separate file.

**Versioning (line level, complementing §8)**
- A new package starts at `0.0.0`; major = breaking only (*"it is not a major change but a
  minor one — major changes are breaking changes"*); pick `patch`/`minor`/`major` by impact,
  not by habit; the changeset description is a release note, so make it descriptive.

---

## 13. How feedback is written

The voice is short, direct, and specific. Match it.

- **Ask "why," don't just assert.** *"why is this needed if reporter already set to
  silent?"*, *"why exactly is the public registry forced?"*, *"What is the benefit of this?"*
  A question that exposes the unjustified change is better than a paragraph.
- **Use GitHub `suggestion` blocks** for exact wording (help text, messages, lists) rather
  than describing the edit.
- **Be honest about uncertainty.** *"I am not sure …"*, *"Maybe we should …"*, *"I don't know
  yet."* Flag a concern you haven't fully verified — say so, and name the scenario that
  worries you.
- **When you decline a suggestion, justify it concretely** — name the invariant it would
  break, the cost it would add, or the measurement that makes it moot. "Declining" replies
  are arguments, not vetoes.
- **When you accept feedback, link the fixing commit** (*"Fixed in `abc1234`"*) and reply on
  the thread before resolving it.
- Don't nitpick what linting already covers — *"We have linting configured and eslint did not
  fail."* Let the tools own style.

Useful comment shapes: *"I don't think this is needed because …"* · *"This will break …"* ·
*"This should be opt-in; don't change the default."* · *"This belongs in the resolver, not
here."* · *"Can we have an e2e test that verifies this?"* · *"This is test-only, so no
changeset is needed."* · *"This PR is already big; please do that in a separate PR."*

---

## 14. Reviewer's checklist

For each PR, in order:

1. **Should it exist?** Real, current, in scope, right branch, not superseded, not better
   solved by an existing mechanism, worth the cost/risk. (§1)
2. **Security.** Walk the `AGENTS.md` checklist against the diff; explain any exploit path;
   don't overreact to non-exploitable advisories. (§5)
3. **Performance.** Hot path or pitched as perf? Is there evidence at realistic scale? (§6)
4. **Scope.** Every touched file justified; unrelated/dangerous changes split out. (§2)
5. **Layer & reuse.** Logic in the owning layer; no reimplementation of existing
   utilities; no abstraction that doesn't reduce coupling. (§4)
6. **Product/contract.** npm-recognized command semantics; defaults preserved or properly
   gated; settings in `pnpm-workspace.yaml`; features documented; no log noise. (§3)
7. **Lockfile format** changes justified and backward-stable. (§4)
8. **Tests.** Right level, meaningful, regression-proving, cross-platform where relevant. (§7)
9. **Changeset.** Present iff user-visible; `"pnpm"` included; one per change; accurate. (§8)
10. **Parity.** pacquet side handled or explicitly deferred. (§9)
11. **Conventions.** `PnpmError`, no swallowed errors, good names, reused libraries, correct
    dependency placement, config through options, no unrelated churn. (§12)

A change is mergeable when it is the **smallest correct, secure, in-scope version of a thing
pnpm should do**, in the right layer, proven by a meaningful test, documented if user-visible,
and mirrored in pacquet where applicable.

---

## 15. Representative review threads

- Rejected re-adding Yarn `resolutions`; deprecation message was enough —
  [#11941](https://github.com/pnpm/pnpm/pull/11941#issuecomment-4740765822)
- Dry-run must behave like real install minus writes —
  [#12270](https://github.com/pnpm/pnpm/pull/12270#issuecomment-4717095101)
- A dry-run-affecting setting would break all dry-run commands; belongs in workspace config —
  [#12080](https://github.com/pnpm/pnpm/pull/12080#issuecomment-4726690874)
- Security hardening gated on a performance design (per-lockfile cache) before default —
  [#11583](https://github.com/pnpm/pnpm/pull/11583#issuecomment-4463511857)
- 2000–3000 packages is a realistic *small*-project benchmark scale —
  [#11583](https://github.com/pnpm/pnpm/pull/11583#issuecomment-4462989559)
- Policy checks belong in git/tarball resolvers via ResolutionVerifiers —
  [#11805](https://github.com/pnpm/pnpm/pull/11805#issuecomment-4508128892)
- Redundant import-state check; `package.json` written last is already the marker —
  [#11170](https://github.com/pnpm/pnpm/pull/11170#issuecomment-4181025884)
- Concurrent same-file writes must be tested, especially on Windows —
  [#11087](https://github.com/pnpm/pnpm/pull/11087#issuecomment-4121103840)
- Dangerous optimizations split into one PR each —
  [#11083](https://github.com/pnpm/pnpm/pull/11083#issuecomment-4120809436)
- Use npm's already-recognized command name; no extra aliases —
  [#11064](https://github.com/pnpm/pnpm/pull/11064#issuecomment-4107201643)
- Changeset required for user-visible; skipped for test-only/internal —
  [test-only](https://github.com/pnpm/pnpm/pull/12398#discussion_r3409452082) ·
  [internal](https://github.com/pnpm/pnpm/pull/12441#discussion_r3435784965)
