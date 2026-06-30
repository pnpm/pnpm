<!-- A maintainer will only start reviewing once the automated code reviewers
(CodeRabbit and Qodo) have approved and CI is green. Please wait for those to
pass before expecting human review. -->

## Summary

<!-- Describe what this PR changes and why, for reviewers. Link any related
issues here (Closes pnpm/pnpm#123, Fixes pnpm/pnpm#456, or Related to
pnpm/pnpm#123 for a partial fix). -->

## Squash Commit Body

<!-- This PR will be squash-merged, using the PR title as the commit subject.
Provide the body of that commit below.

This body is for developers reading the git history, so be as detailed as the
change warrants: explain the rationale, design decisions, and trade-offs. The
user-facing release note belongs in the changeset, not here. -->

```text
Explain what changed and why. Reference issues here too (Closes pnpm/pnpm#123).
```

## Checklist

<!-- Mark items with [x] once done. Remove items that don't apply. -->

- [ ] The change is implemented in both the TypeScript CLI and the Rust
  `pacquet/` port, or the description notes what still needs porting.
- [ ] Added a changeset (`pnpm changeset`) if this PR changes any published
  package. Keep it short and written for pnpm users — it becomes a release note.
- [ ] Added or updated tests.
- [ ] Updated the documentation if needed.
