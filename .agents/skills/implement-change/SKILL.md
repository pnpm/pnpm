---
name: implement-change
description: Implement a pnpm feature, bug fix, or refactor by checking existing capabilities, prioritizing code reuse and deduplication, assessing architecture impact, and validating the final change. Use when asked to implement a change in this repository.
---

# Implement a change

Apply [PHILOSOPHY.md](../../../PHILOSOPHY.md),
[AGENTS.md](../../../AGENTS.md), and the relevant product instructions and
style guides. Keep repository policy in those sources; this skill connects
implementation to the [testing-changes](../testing-changes/SKILL.md) and
[review-code](../review-code/SKILL.md) workflows.

## Understand the problem before adding code

Establish the intended behavior and concrete success criteria from the user's
request, reproduction, or specification. Determine which products and versions
are affected using the repository's development policy. Reproduce bugs before
choosing a fix when practical.

Always assess whether existing pnpm features can solve the problem, including
combinations of commands, configuration, hooks, and workspace capabilities.
Check their actual behavior against the success criteria. Prefer an existing
capability when it fully solves the problem; explain how to use it and any
limits. If it only partly solves the problem, identify the gap and extend the
owning feature when that produces a cohesive design. Do not add a parallel
feature merely because the request suggests a new setting or command. Respect
an explicitly requested behavior that existing capabilities do not provide.

## Assess the architecture

Before adding any new feature, analyze its effect on the overall architecture,
not just the file being edited. Trace the affected data flow and responsibilities
across commands, configuration, resolution, storage, or other relevant layers.
Identify where the behavior belongs, which existing abstractions it extends,
and how it interacts with related features and shared state or formats.

Summarize the architectural fit and meaningful tradeoffs before substantial
implementation. Consider whether the change adds competing sources of truth,
blurs a boundary, or introduces special cases other features will need to
understand. Prefer a coherent extension over a local workaround. Keep necessary
refactoring bounded to the change; architectural analysis is not authorization
for an unrelated redesign.

## Prioritize reuse and deduplication

Search for similar behavior, helpers, and tests before writing non-trivial
logic. Inspect the relevant product and shared utilities, including callers of
candidate helpers. Prioritize removing duplication over adding another copy.

If the needed logic already exists, reuse it. If it is not reusable yet,
extract the common behavior into the appropriate shared helper or package and
update the affected callers. When consolidating stable features would break
compatibility, temporary duplication may remain until the next major version;
identify the intended shared abstraction and consolidation point. Experimental
features can be consolidated immediately. Apply the same check to similar
logic introduced by the change. Preserve meaningful differences in contracts; do not force
unrelated behavior into a generic abstraction merely because the code looks
alike. Follow the repository's dependency placement and library reuse rules.

## Implement and validate

Implement the smallest cohesive change that satisfies the intended behavior.
Add tests that exercise its observable contract and relevant interactions,
and update documentation and changesets when repository policy requires them.

Use [testing-changes](../testing-changes/SKILL.md) to select and run appropriate
checks. Investigate failures. Then use [review-code](../review-code/SKILL.md) on
the complete diff, fix verified findings, and rerun checks affected by the fixes.
Reassess reuse and architectural fit against the final implementation.

Report what changed, existing capabilities considered, the reuse or extraction
chosen, material architectural implications, and validation limits. Scale the
explanation to the change. Commit, push, or open a PR only when the user or
calling workflow authorizes it; use [pull-requests](../pull-requests/SKILL.md)
when taking the change through a PR.
