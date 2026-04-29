# User Queue Item 4: Suggested Edits/Deletions for PR #11363 Comments

## Comment #2 — https://github.com/pnpm/pnpm/pull/11363#discussion_r3142261542

**Current text** (HaleTom): Argues for keeping `allowBuilds == null` and against Copilot's `!hasDependencyBuildOptions` suggestion.

**Problem**: This reply argues for keeping `allowBuilds == null`. In comment #3, you adopted Copilot's `!hasDependencyBuildOptions` suggestion. This reply now contradicts the final code.

**Suggested edit**: Prepend the superseding header, leave original text for context.

```
EDIT: Superseded by [this comment](https://github.com/pnpm/pnpm/pull/11363#discussion_r3142281778)
---

Thanks for the review!
... [rest unchanged] ...
```

GraphQL comment node ID needed. Fetch via:
```bash
gh api graphql -f owner=pnpm -f repo=pnpm -F pr=11363 -f query='
query($owner: String!, $repo: String!, $pr: Int!) {
  repository(owner: $owner, name: $repo) {
    pullRequest(number: $pr) {
      reviewThreads(first: 100) {
        nodes {
          comments(first: 100) {
            nodes {
              id
              databaseId
              author { login }
            }
          }
        }
      }
    }
  }
}'
```

Then update:
```bash
gh api graphql -f query="
mutation {
  updatePullRequestReviewComment(input: {
    pullRequestReviewCommentId: \"PRRC_NODE_ID_HERE\"
    body: \"EDIT: Superseded by [this comment](https://github.com/pnpm/pnpm/pull/11363#discussion_r3142281778)\n---\n\nThanks for the review!\n\n**On the code suggestion**: The proposed \`!hasDependencyBuildOptions(pnpmConfig)\` check changes the condition from \`allowBuilds == null\` to the absence of **any** build-related config key. This would silently defer to other build options (like \`dangerouslyAllowAllBuilds\` from \`.npmrc\`) during the GVS defaulting phase. This changes the design semantics: even with \`dangerouslyAllowAllBuilds\`, the GVS layer intends to lift the build restriction to engine-agnostic hashing by setting an explicit policy (\`allowBuilds = {}\`). The root bug was the **ordering** relative to \`globalDepsBuildConfig\` re-application, not the condition logic. Keeping \`allowBuilds == null\` ensures the GVS default is only applied when **no** build policy exists, preserving the original design intent.\n\n**On the regression test**: Agreed — will add a test in \`config/reader/test/index.ts\` verifying that with GVS on, no workspace manifest, and \`.npmrc\` \`dangerously-allow-all-builds=true\`, the resulting config retains \`dangerouslyAllowAllBuilds\` and \`allowBuilds\` is set to \`{}\` (not skipping builds).\"
  }) {
    comment { id }
  }
}"
```

---

## Comment #3 — https://github.com/pnpm/pnpm/pull/11363#discussion_r3142281778

**Current text** (HaleTom): "Applied both suggestions — thank you!"

**Problem**: "Applied both suggestions" is inaccurate. In comment #2 you argued against the code suggestion. You later reconsidered and adopted it. The word "both" implies acceptance from the start.

**Suggested edit**: More accurate language.

```
Reconsidered and adopted both suggestions — thank you!

**Code suggestion (`!hasDependencyBuildOptions`):** On further reflection, this makes the GVS defaulting condition consistent with the re-application guard on the line immediately before it. Now `allowBuilds = {}` only applies when no build policy is configured at all — not even `dangerouslyAllowAllBuilds` — which is the correct semantics.

**Regression test:** Added in config/reader/test/index.ts — verifies that `dangerouslyAllowAllBuilds` from .npmrc/config.yaml is preserved when `global=true + enableGlobalVirtualStore=true` and no global pnpm-workspace.yaml exists.

Pushed as 475ef6b on branch fix-global-allow-builds.
```

Same mutation as above with the updated body and the correct PRRC node ID for comment 3142281778.

---

## Review #7 — https://github.com/pnpm/pnpm/pull/11363#pullrequestreview-4175871906

**Current state**: HaleTom `COMMENTED` review with empty body.

**Problem**: Empty review envelope — likely created as side effect of `addPullRequestReviewThreadReply` leaving a pending review that submitted empty.

**Suggested action**: Delete.

Fetch the GraphQL node ID for review 4175871906:
```bash
gh api graphql -f owner=pnpm -f repo=pnpm -F pr=11363 -f query='
query($owner: String!, $repo: String!, $pr: Int!) {
  repository(owner: $owner, name: $repo) {
    pullRequest(number: $pr) {
      reviews(first: 10) {
        nodes {
          id
          databaseId
          state
          body
          author { login }
        }
      }
    }
  }
}'
```

Then delete:
```bash
gh api graphql -f query='
mutation {
  deletePullRequestReview(input: {
    pullRequestReviewId: "PRR_NODE_ID_HERE"
  }) {
    pullRequestReview { id }
  }
}'
```

---

## Review #8 — https://github.com/pnpm/pnpm/pull/11363#pullrequestreview-4175892460

**Current state**: Second empty HaleTom `COMMENTED` review.

**Suggested action**: Delete (same mutation, correct node ID for review 4175892460).

---

## Summary

| Item | Action | Reason |
|------|--------|--------|
| #2 (discussion_r3142261542) | Prepend EDIT superseded header | Contradicts later reply #3; superseded format preserves history |
| #3 (discussion_r3142281778) | Minor text edit | "Applied both" → "Reconsidered and adopted both" |
| #7 (review 4175871906) | Delete | Empty zero-body COMMENTED review — noise |
| #8 (review 4175892460) | Delete | Empty zero-body COMMENTED review — noise |
