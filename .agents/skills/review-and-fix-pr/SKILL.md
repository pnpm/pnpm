---
name: review-and-fix-pr
description: Review and fix an existing pnpm pull request, rebase it, commit and push fixes, then follow CI and review to completion. Use when asked to review and fix a PR or launched by the git-wt PR hook.
---

# Review and fix an existing PR

The supplied PR already exists. Work on its branch, commit and push the fixes,
and follow it until checks are green and the review round has nothing left to
act on. Do not pause for the user's review of local fixes.

Read the [pull-requests skill](../pull-requests/SKILL.md) and use its workflow
from "After every push" onward, including its guidance for failing checks,
conflicts, PR descriptions, and review replies. Skip PR creation and draft
management; preserve the existing PR's draft or ready status. This workflow
does not authorize merging the PR.

## Review and fix

1. Use `gh` to read the PR description, full diff, issue comments, review bodies,
   and inline review threads. Understand the change's intent and verify each
   finding before acting on it.
2. Rebase onto the latest base branch with
   `./.agents/skills/pull-requests/scripts/resolve-pr-conflicts.sh <pr>`. Run this even if GitHub reports no
   conflicts. The script force-fetches the base and pushes the rebased branch.
   If it reports `MANUAL_RESOLUTION_NEEDED`, resolve and stage the listed files,
   then run `./.agents/skills/pull-requests/scripts/resolve-pr-conflicts.sh <pr> --continue`. Re-read the diff
   after rebasing. A `git-wt` checkout may be named `pr-<pr>`; the script can
   switch it to the PR's actual head branch.
3. Address verified review findings and review the full change against
   [REVIEW_GUIDE.md](../../../REVIEW_GUIDE.md), with security first and
   performance second. Apply the repository and relevant product instructions
   and style guides. Check version coverage and changeset requirements.
4. Use the [testing-changes skill](../testing-changes/SKILL.md) to select and
   run the checks covering your fixes. Investigate and fix failures.
5. Ensure the repository's git hooks are installed as required by `AGENTS.md`.
   Commit your fixes with a Conventional Commit message and push to the PR's
   head branch. Preserve any unrelated local changes.

## Finish the existing PR

Every push, including the rebase script's push, starts another CI and review
round. Follow the pull-requests skill's loop, verify new findings, commit and
push corrections, and reply to and resolve review threads after the fixes are
on the remote branch. Keep the PR title and description accurate and sign
agent-authored GitHub content as required by that skill.

Finish only when the checks are green and the reviewers have reported on the
current head with nothing left to act on. If an external prerequisite prevents
completion, report it concretely. Summarize the fixes, conflicts resolved,
findings declined and why, validation, and final CI and review status.
