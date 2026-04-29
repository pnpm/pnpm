# User Queue Item 2: Skill Update Patches for github-pr-review-handling

## Patch 2a — Add "Comment Types: Threaded vs Flat" section

Target: `~/.config/opencode/skills/github-pr-review-handling/SKILL.md`
Insert after "## When NOT to Use" (line 22), before "## Tool Preference".

```patch
--- a/SKILL.md
+++ b/SKILL.md
@@ ## When NOT to Use+
+## Comment Types: Threaded vs Flat
+
+GitHub PRs have two distinct comment surfaces. Determining which one you're dealing with is critical for correct reply behaviour and quoting.
+
+| Property | Threaded (inline review) | Flat (PR-level) |
+|----------|--------------------------|------------------|
+| GraphQL parent | `reviewThreads.nodes` | `comments.nodes` |
+| Has `PRRT_` thread ID? | Yes | No |
+| Has `path`/`line`? | Yes (file context) | No |
+| Has resolve/unresolve? | Yes | No |
+| REST `in_reply_to_id`? | Works within thread | Not applicable |
+| Reply mutation | `addPullRequestReviewThreadReply` | Normal comment creation |
+| Visual grouping | Threaded under the inline comment | Flat chronological list |
+
+**Detection rule**: A comment is threaded if and only if it appears in `reviewThreads.nodes[].comments.nodes`. A comment is flat if and only if it appears in PR-level `comments.nodes` (issue comments). The two sets are disjoint.
+
+**Why it matters**: Threaded comments have inherent visual context (they're grouped under the original inline comment). Flat comments have no such context — readers cannot tell which flat comment replies to which.
+
 ## Tool Preference
```

## Patch 2b — Add "Quoting Rules" section

Target: `~/.config/opencode/skills/github-pr-review-handling/SKILL.md`
Insert after the new "Comment Types" section, before "## Anti-Patterns".

```patch
--- a/SKILL.md
+++ b/SKILL.md
@@ ## Anti-Patterns+
+## Quoting Rules
+
+### Flat (PR-level) comments
+
+Flat comments have no thread structure. When replying to a flat comment:
+- Always quote the relevant part of the message you're responding to
+- Use `@username` to tag the person being replied to
+- Format:
+  ```
+  > quoted text
+
+  @username your response
+  ```
+- Even if only one other comment exists, quote it — a future reader may not see them adjacent
+
+### Threaded (inline review) comments
+
+Threaded comments have visual grouping context. When replying in a thread:
+- If your reply is **directly underneath** the message it's replying to (immediate next in thread): no quote needed
+- If your reply is **not directly underneath** (e.g., another reply intervenes, or you're replying to an earlier comment further up the thread): quote the relevant text and `@username` the person
+- Format when quoting needed:
+  ```
+  > @username: quoted excerpt
+
+  your response
+  ```
+
 ## Anti-Patterns
```

## Patch 2c — Add "Guard Against Empty Review Submissions" to Anti-Patterns

Target: `~/.config/opencode/skills/github-pr-review-handling/SKILL.md`
Insert at end of Anti-Patterns section (after line 37, before "## Mandatory").

```patch
--- a/SKILL.md
+++ b/SKILL.md
@@ ## Mandatory: Check All PR Feedback Surfaces+
+**NEVER leave empty review submissions.** When using `addPullRequestReviewThreadReply`, the mutation may create a pending review envelope. After posting all replies in a batch:
+1. Query `reviews(first: 10)` on the PR
+2. Check for reviews with `state: COMMENTED` and empty `body`
+3. If found and authored by you, either submit them with a summary body or dismiss them via `deletePullRequestReview`
+4. Do not leave zero-body `COMMENTED` reviews on the public record — they're noise
+
 ## Mandatory: Check All PR Feedback Surfaces
```

## Patch 2d — Add "Superseding Your Own Earlier Argument" to Anti-Patterns

Target: `~/.config/opencode/skills/github-pr-review-handling/SKILL.md`
Insert after the empty-review anti-pattern, before "## Mandatory".

```patch
--- a/SKILL.md
+++ b/SKILL.md
@@ ## Mandatory: Check All PR Feedback Surfaces+
+**NEVER leave self-contradictory replies in the same thread.** If you post a reply that contradicts or supersedes your own earlier reply in the same thread, update the earlier reply via `updatePullRequestReviewComment` to mark it as superseded. Leaving contradictory text on the public record confuses future readers.
+
+Superseded-reply edit format (first lines of the edited comment):
+```
+EDIT: Superseded by [this comment](URL_OF_THE_NEWER_REPLY)
+---
+
+{original text remains below, unchanged}
+```
+
+Do not delete the original text — it preserves conversation history. The EDIT header + link points the reader to the current position.
+
 ## Mandatory: Check All PR Feedback Surfaces
```

## Patch 2e — Add "Honesty in Public PR Comments" to Review Reply Principles

Target: `~/.config/opencode/skills/github-pr-review-handling/SKILL.md`
Insert as new principle #5, after principle #4.

```patch
--- a/SKILL.md
+++ b/SKILL.md
@@ 4. **Push back where it's warranted, adopt where it's valid.**
+
+5. **Be honest and humble in public.** There is no shame in calling out your own mistakes — doing so builds trust and reduces confusion. When correcting your own error, be direct and matter-of-fact. When correcting someone else's error, be tactful, considerate, and kind — but still point it out if the error makes a material difference to the conversation (e.g., could cause confusion to others). It's not about being right; it's about being humble, admitting mistakes, and politely correcting others if that reduces confusion or future errors. Use plain technical language, not dramatic or self-deprecating phrasing.
```

## Patch 2f — Add common-mistake entries

Target: `~/.config/opencode/skills/github-pr-review-handling/SKILL.md`
Append to the "Common Mistakes" table.

```patch
--- a/SKILL.md
+++ b/SKILL.md
@@ | Not outputting the post URL after posting | Always give the URL of the post. For PRs, give the title + URL. For thread comments, give the comment URL. |
+| Leaving empty COMMENTED reviews on the PR | After batch posting replies, check for zero-body reviews and dismiss or submit them. |
+| Leaving self-contradictory replies in a thread | Update the earlier reply with an EDIT header pointing to the newer reply. |
+| Not quoting in flat PR-level comments | Flat comments have no thread context — always quote + @username when replying. |
+| Not quoting in threaded comments when reply is non-adjacent | If another reply intervenes between you and the target, quote the relevant text + @username. |
```
