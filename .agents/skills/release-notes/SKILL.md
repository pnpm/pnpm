---
name: release-notes
description: Curate a pending release page - merging entries that describe one change, dropping notes for defects that never shipped, ordering by importance, and checking the version the intents ask for. Use when reviewing a release PR, a generated changelog, or the notes for a specific version.
---

# Release notes

A release page is read by pnpm users, most of them skimming for the one entry that
affects them. It is not a log of the pull requests that landed. Every entry has to
earn its line.

The wording rules for a single entry live in the "Changeset style" section of
`CLAUDE.md`. This skill is about the set: which entries exist at all, how they are
grouped, what order they appear in, and how the page reads end to end.

## Edit the composed section, not the intents

1. Each merged pull request leaves a `.changeset/<id>.md` intent: a frontmatter map
   of package to bump type, then the prose.
2. `pnpm run bump` on the release branch consumes the pending intents, **deletes
   them**, and composes them into `.changeset/changelogs/<package>@<version>.md`,
   one section per released package, grouped into Major, Minor, and Patch changes.
3. That file is the release page. The Rust CLI's GitHub release body is literally
   `tail -n +2 .changeset/changelogs/pacquet@<version>.md` plus the sponsors
   fragment (`.github/workflows/release.yml`), and the same section is composed into
   the published `CHANGELOG.md` and the release blog post.

So curate `.changeset/changelogs/<package>@<version>.md` directly, on the release PR
branch, after the bump has written it. The intents are gone by then, so the composed
section is the only surviving copy - there is nothing to keep in sync. Editing it is
also the only way to reorder or merge entries: the composer emits them in
alphabetical order of the intent filename and offers no other lever.

Two things this does not cover:

- **Bump types still live in the intents.** If the planned version is wrong, fix the
  intent's frontmatter and re-run the bump. That is the only reason to touch an
  intent at this stage.
- **A published version's section is frozen.** `confirmed_published_versions` garbage
  collects a parked section only when the published changelog contains it verbatim,
  so editing one after release breaks the match forever. Before publish, it is the
  right place to edit; after, corrections go in a new changeset.

Curation lives on `release-pr/<target>`, which the Create Release PR workflow rebuilds
from the target branch on every dispatch. A re-dispatch discards it. Curate when the
release is actually going out.

## What to cut

**Defects introduced and fixed between two releases.** If the bug never existed in a
published version, no user can recognise it, and the entry only advertises that the
feature arrived broken. To decide, find the release commit for the package's previous
version (`git log --grep 'chore(release)'`) and check whether the code the fix touches
predates it. A follow-up to a feature shipping in this same release is the usual case.

**Internals with no observable consequence.** Refactors, renamed modules, "now uses X
internally". If you cannot finish the sentence "so you can ..." or "so it no longer
...", drop it.

**Mechanism the reader will not act on.** Keep the sentence that names the command,
setting, or symptom. Cut the one explaining which cache, tier, or syscall changed,
unless the reader has to configure around it.

## What to merge

Merge entries that describe one change as the user experiences it, even when they
came from different pull requests:

- Same command or setting, same defect (two `pnpm dedupe` selection bugs).
- One symptom with several causes (install failing on Android for a TLS reason and a
  permissions reason).
- A feature and the follow-ups that completed it (a container registry plus its size
  limits, token scopes, and blob deletion).
- A cluster of speedups with one headline (`Sped up repeat installs.` then the
  specifics).

Two entries that mention the same setting from different angles are duplication even
when both are accurate. Say it once, in the entry the reader will find first.

## Lead the page

Deno opens a release post with one sentence naming the top few changes: "Deno 1.28
ships with stabilized npm modules, auto-discovered lock file, a new subprocess API,
and more." A reader decides from that line whether to read on.

Write that line as a paragraph directly under the `## <version>` heading, before the
first `###` section. `tail -n +2` keeps it, so it becomes the opening line of the
GitHub release body, and it sits under the version heading in `CHANGELOG.md`. Name
three or four things at most, in the words the entries below use.

## Ordering

The published page has no headings other than Major, Minor, and Patch. Thirty patch
bullets arrive as one undifferentiated list, so the order is the whole of the page's
structure. Projects that categorise explicitly publish the ranking you want: Gitea
orders BREAKING, SECURITY, FEATURES, PERFORMANCE, ENHANCEMENTS, API, BUGFIXES; uv
splits Enhancements, Performance, Bug fixes, Preview features, Breaking changes.
pnpm's composer emits no such headings, so **build those blocks out of the order**:

1. Breaking changes and anything the reader must act on.
2. Security fixes. Gitea puts these second for a reason; a reader who stops after two
   entries must not have missed one. Lead the section they fall in, and say in the
   lead paragraph that the release carries one.
3. Installs or commands that fail outright, and anything that damaged files.
4. Wrong results: a dependency resolved, linked, or skipped incorrectly.
5. Speed and resource use.
6. Behavior corrections in one command or setting.
7. Output: messages, warnings, help text.

The Major, Minor, and Patch headings cut across that ranking and you cannot reorder
past them, so a security fix carrying a patch bump still lands below every feature.
That is what the lead paragraph is for.

Within a section, group: all the speedups together, all the `pnpm dedupe` fixes
together, the Windows items together. A reader who hits three consecutive entries
about output messages knows the interesting part is over.

**Past about a dozen entries in one section, name the groups.** Add `####` subject
headings under the `###` bump heading, ordered by the ranking above:

```markdown
### Patch Changes

#### Installing packages

- ...

#### Resolving and linking dependencies
```

Aim for four to eight groups and at least two entries in each; a group of one is
noise, so fold it into its neighbour. Name them for what the reader was doing when
they hit the bug (Installing packages, Running scripts and tasks, Windows), not for
the subsystem that changed. Leave a section of a dozen or fewer flat - the headings
cost more than they save.

Both consumers tolerate the extra level: `release.yml` writes the Rust release body
with `tail -n +2`, and `getChangelogEntry` (`pnpm11/__utils__/get-release-text`)
slices between headings of the *same* depth as `## <version>`, so a `####` heading
neither ends the slice nor is dropped. It does scan every heading for
`major|minor|patch`, so avoid a group literally named "Patching" - call it "Patched
dependencies".

In the Minor section of a feature release, lead with the three to five entries that
change how people work, the way Playwright leads with a handful of highlights before
its flat API list. Largest diff is not the same as most important. For the one or two
biggest, TypeScript's shape is worth copying: open with the problem the reader
recognises, then the new capability, then the code.

## How one entry should read

**Do not ship the raw composed list.** Yarn's release pages are the conventional-commit
log with the PR appended - "fix(nm): prefer direct dependency binaries by @user in
#1234". Every entry is accurate and the page tells a user nothing: it is indexed by
the change's author, not by the reader's problem. pnpm's uncurated output has the same
shape, one entry per pull request in filename order. That is the thing being fixed
here.

**The first sentence is the title.** esbuild gives every entry a bold heading; uv
makes each bullet short enough to be one. pnpm's format has no title slot, so the
opening clause carries it: name the command, setting, or platform in the first three
words, then the outcome. "`pnpm install` no longer fails with ..." works. "Fixed an
issue where, under certain conditions, ..." does not.

**Default to one sentence.** uv's bullets run 80 to 120 characters and hold exactly
one idea; Gitea's run 60 to 90. SQLite states a whole behavior change in one clause
and no adjectives: "Fix the count-of-view optimization so that it does not give an
incorrect answer for a DISTINCT query." Add a second sentence when the reader needs the old behavior to recognise
the bug, and a second paragraph only when they must act: a migration, an opt-out, an
exception to what the first paragraph promised.

**Show the literal change when output changes.** esbuild pairs almost every entry
with before and after. Where a change rewrites a manifest range, a lockfile field, or
a timestamp, print both: `pnpm update react@19.3.0` on `"react": "^19.2.8"` writes
`"react": "^19.3.0"`. Naming the shape beats describing it.

**Name what the reader can act on.** Every uv bullet names a flag, a file, or a
platform. If an entry mentions no command, setting, file, error code, or platform,
the reader cannot tell whether it applies to them.

**Link the issue, not the pull request.** The issue shows the reader the report they
may have filed. Use `[#14722](https://github.com/pnpm/pnpm/issues/14722)` and put it
at the end of the sentence it belongs to, not the end of the entry. Keep the link
style identical across every entry in a release.

## Check the version the intents ask for

`pnpm change status` prints the planned version for every package before anything is
written. Read the line for each released package and ask whether it matches the
entries you are about to curate. Read the packages that carry no intent of their own
too: `@pnpm/napi` rides pacquet's `versioning.fixed` group, and `release.yml` fails
the build if the two versions have drifted apart.

The release engine applies plain semver: a `major` intent on a `0.x` package moves it
to `1.0.0`, and on a prerelease lane that resets the counter (`0.1.0-alpha.10` to
`1.0.0-alpha.0`). There is no "0.x majors are minors" rule. Before accepting a major
bump, check that the breaking change breaks something a published version could do. A
new URL layout for a capability that ships in the same release breaks nothing.

## Finish

Read the curated section top to bottom, as a user would. It is done when:

- No two entries make you check whether they are the same change.
- Every entry's first line tells you whether it applies to you.
- The order stops surprising you: you can tell where the important part ended.
- Nothing describes a bug that no published version ever had.
- An empty `## <version>` section is not being shipped for a package that only bumped
  because it rides a fixed group. Give it an entry or accept the blank heading
  knowingly.
