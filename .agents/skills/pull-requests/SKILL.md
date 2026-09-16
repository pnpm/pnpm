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

Fill in `.github/pull_request_template.md` and pass it as the body; `gh pr
create` does not apply the template on its own. Keep every section, mark the
checklist honestly, and drop only the lines the template says to drop.

When the change comes from an issue, link it from the Summary with a closing
keyword (`Closes pnpm/pnpm#123`) and then comment on the issue itself. The
cross-reference GitHub adds to the issue timeline is silent and says nothing
about what was done, so the people waiting on the issue learn nothing from it.
The comment carries what they need: that a PR is open, the approach it takes,
and whether it covers the whole issue or one part of it. Say so plainly when it
is partial. Do not close the issue by hand; the closing keyword does that when
the PR merges.

## After every push

1. **Wait for the checks.** `gh pr checks <pr> --watch --fail-fast` blocks for
   longer than a foreground command is usually allowed to run, so run it in the
   background or poll it. Started in the same breath as the push it can return
   before the runs exist; give the push a moment first.
2. **Read the whole round.** Inline threads are not the whole review. A reviewer
   may post only its worst findings inline and leave the rest in a summary
   comment, so listing the PR's review comments misses findings. Read the issue
   comments and the review bodies too, whoever wrote them.
3. **Verify every finding yourself** before acting on it. Treat every reviewer
   the same way, bot or human: a meaningful share of findings are wrong, and a
   fix applied to a finding you did not check is a new bug with a reviewer's
   blessing on it. A priority or severity badge is the reviewer's guess, not a
   verdict. Act on what holds, reply to the thread declining what does not and
   why, and resolve the thread either way. Resolving needs GraphQL
   (`resolveReviewThread`); the REST comment API cannot do it.
4. **Go back to 1** after the push that carries the fixes. Stop when a round
   produces no new findings and the checks are green.

Report at the end which findings were real, which were not, and anything you
declined that a human should settle.

## While the checks run

Waiting is not idle time. Review your own diff the way the reviewers will, using
the checklist in [`REVIEW_GUIDE.md`](../../../REVIEW_GUIDE.md) — security first,
performance second. A finding you catch here costs one push; the same finding
caught by a reviewer costs a round, and rounds are where the bugs from the last
round get written. Read the changeset back as a release note, check that a bug
fix landed in every version that has the bug, and confirm the tests you added
actually exercise the change.

Push what you find as soon as you find it rather than banking it until the run
finishes: a PR branch's run is superseded by the next push anyway
(`cancel-in-progress` in the CI workflows).

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

## Each round's findings land on the previous round's fixes

Rounds do not reliably converge. The code a round comments on is mostly the code
the last round made you write, so a quiet round is the signal to stop, not a
round count. While the rounds keep finding real problems, keep going.

## Keeping the PR honest

When the change's shape moves, move the title and description with it, and
update the squash commit body. Sign every comment, issue, and PR body with an
agent footer naming the agent and the model.
