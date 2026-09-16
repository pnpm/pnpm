---
name: pull-requests
description: Take a change through a pull request in the pnpm repository — opening it, then staying with it after every push until CI is green and the review round is quiet. Use when opening a PR, after pushing to one, when a check fails, or when review comments arrive.
---

# Pull requests

Pushing is the middle of the task. The PR is done when the checks pass and a
review round produces nothing left to act on. Until then the work is yours.

Several bots review this repository, and they re-review on every push. A push
therefore always buys a new round, and the round arrives minutes later, after
the turn that pushed would otherwise have ended. Stay for it.

## Opening

Open it as a draft (`gh pr create --draft`). Fill in
`.github/pull_request_template.md` and pass it as the body; `gh pr create` does
not apply the template on its own. Keep every section, mark the checklist
honestly, and drop only the lines the template says to drop.

When the change comes from an issue, link it from the Summary with a closing
keyword (`Closes pnpm/pnpm#123`) and then comment on the issue itself. The
cross-reference GitHub adds to the issue timeline is silent and says nothing
about what was done, so the people waiting on the issue learn nothing from it.
The comment carries what they need: that a PR is open, the approach it takes,
and whether it covers the whole issue or one part of it. Say so plainly when it
is partial. Do not close the issue by hand; the closing keyword does that when
the PR merges.

## Draft until the change is worth reading

CI runs on a draft here; the reviewers do not. That asymmetry is the whole point
of opening as one. A failing lint job, a test you forgot to update, a bug your
own review pass catches — in draft each of those costs a push and nothing else,
while on a ready PR each one burns a review round and leaves a thread behind.

So stay in draft until the checks are green and your own pass over the diff is
clean, then `gh pr ready <pr>`. The first round of review then reads a finished
change instead of a half-fixed one, and its findings are about the design rather
than the leftovers.

Do not leave a draft behind when you stop. A draft with no note on it reads as
abandoned: either mark it ready or say in a comment what is left to do.

## After every push

This is the loop for a PR that is ready for review. Steps 1 and 4 apply while it
is still a draft too; the review steps start when you mark it ready.

1. **Wait for the checks.** `gh pr checks <pr> --watch` blocks for longer than a
   foreground command is usually allowed to run, so run it in the background or
   poll it. Started in the same breath as the push it can return before the runs
   exist; give the push a moment first. Watch to the end rather than stopping at
   the first failure: the push that fixes it cancels whatever was still running,
   so a second failure you never waited for costs another full cycle.
2. **Read the whole round.** Inline threads are not the whole review. A reviewer
   may post only its worst findings inline and leave the rest in a summary
   comment, so listing the PR's review comments misses findings. Read the issue
   comments and the review bodies too, whoever wrote them.
3. **Verify every finding yourself** before acting on it. Treat every reviewer
   the same way, bot or human: a meaningful share of findings are wrong, and a
   fix applied to a finding you did not check is a new bug with a reviewer's
   blessing on it. A priority or severity badge is the reviewer's guess, not a
   verdict. Act on what holds. Reply on every thread either way, naming the
   commit that fixed it or the reason you are not acting, and then resolve it:
   the thread is the record a reviewer checks each fix against. Reply once that
   commit is on the remote branch, never before — a hash read off a local commit
   that a rebase or an amend then rewrites names something nobody can look up. Resolving needs
   GraphQL (`resolveReviewThread`); the REST comment API cannot do it.
4. **Bring the title and description with the code.** Reread them after any push
   that changes what the PR does, and edit them when they no longer describe it
   (`gh pr edit --title`, `--body`). This is not housekeeping: the title becomes
   the squash commit's subject and the template's Squash Commit Body section
   becomes its message, so whatever is stale at merge time is what lands in the
   history for good.
5. **Go back to 1** after the push that carries the fixes.

A round is finished only when every reviewer's summary comment names your head
commit; each one says which commit it reviewed. Green checks and an empty
comment list prove nothing on their own, because a round that has not started
yet looks exactly like one that found nothing. Stop when every reviewer has
reported on the head commit, none of them found anything left to act on, and the
checks are green.

Report at the end which findings were real, which were not, and anything you
declined that a human should settle.

## While the checks run

Waiting is not idle time, and in the draft phase this pass is the whole job.
Review your own diff the way the reviewers will, using
the checklist in [`REVIEW_GUIDE.md`](../../../REVIEW_GUIDE.md) — security first,
performance second. A finding you catch here costs one push; the same finding
caught by a reviewer costs a round, and rounds are where the bugs from the last
round get written. Read the changeset back as a release note, check that a bug
fix landed in every version that has the bug, and confirm the tests you added
actually exercise the change.

Push what you find as soon as you find it rather than banking it until the run
finishes. The main CI workflows cancel a PR branch's in-progress run when the
next push lands, so waiting for a run you are about to supersede buys nothing.
Send the fixes in one push, though: a push also restarts a review that is in
flight, and a trickle of pushes restarts it over and over.

## Failing checks

Never write a failure off as pre-existing, flaky, or unrelated without evidence
for that claim — `AGENTS.md` makes this a repo rule, and the first guess is
usually wrong. Pull the failing log (`gh run view <run> --log-failed`),
reproduce it locally with the selection from the
[`testing-changes`](../testing-changes/SKILL.md) skill, and fix the cause.

A fork PR's runs can sit awaiting approval and then show up cancelled: a
scheduled sweep cancels approval-held workflows after 30 minutes
(`.github/workflows/cancel-unapproved-workflows.yml`). That is not a test
failure, and the run has to be approved and restarted.

## Conflicts appear while you wait

A PR that merged cleanly when you opened it stops merging cleanly as soon as
something landing on `main` touches the same lines. Nothing announces this, so
check it each time round the loop: `gh pr view <pr> --json mergeable,mergeStateStatus`.
`CONFLICTING` means rebase; `UNKNOWN` means GitHub has not computed it yet, which
is what you see right after a push, so ask again rather than reading it as a
verdict. `BLOCKED` is about required checks and reviews, not conflicts.

Rebase with `./shell/resolve-pr-conflicts.sh <pr>` (documented under "Resolving
Conflicts in GitHub PRs" in `AGENTS.md`); it resolves a `pnpm-lock.yaml` conflict
by reinstalling and stops with the file list when a conflict needs you.

Rebase rather than merging `main` in: the branch protection on `main` requires
linear history and merge commits are disabled, which is why the script and
GitHub's update-branch button both rebase.

Fold the rebase into the push that carries your fixes when you can, and answer
the threads after that push, not before: the rebase rewrites your fix commits,
so a hash quoted ahead of it names a commit the branch never receives. The force
push also marks the open comments outdated and moves their anchors, which is the
second reason the reply has to carry the hash — the line the comment hangs on no
longer points at the fix.

Re-read the diff after a rebase. A conflict resolved wrongly is a real bug that
arrives with no review comment attached to it.

## Each round's findings land on the previous round's fixes

Rounds do not reliably converge. The code a round comments on is mostly the code
the last round made you write, so a quiet round is the signal to stop, not a
round count. While the rounds keep finding real problems, keep going.

## Keeping the PR honest

Sign every comment, issue, and PR body with an agent footer naming the agent and
the model.
