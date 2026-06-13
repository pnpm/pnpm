// Rejects commit messages that contain a bare `@name` mention — one that is not
// wrapped in backticks. GitHub turns such a token into a real notification.
//
// Rationale lives in the error message below and in AGENTS.md.

import { readFileSync } from 'node:fs'

const msgPath = process.argv[2]
if (!msgPath) {
  console.error('reject-bare-mentions: missing commit message file path argument')
  process.exit(1)
}

const offenders = findBareMentions(scannableText(readFileSync(msgPath, 'utf8')))

if (offenders.size === 0) {
  process.exit(0)
}

reportAndExit(offenders)

// Reduce the raw commit-message file to the text that will actually be
// committed and rendered by GitHub, with code spans neutralised so a mention
// inside backticks is not flagged.
function scannableText (raw) {
  return stripCodeSpans(stripCommentLines(stripScissorsSection(raw)))
}

// `git commit -v` appends the diff below a scissors line; git discards
// everything from that line onward, so the hook must not scan it either —
// otherwise an `@mention` that only appears in the diff (e.g. a scoped-package
// import) would wrongly reject a commit on text the author cannot remove.
function stripScissorsSection (text) {
  const scissors = text.match(/^#\s*-{2,}\s*>8\s*-{2,}.*$/m)
  return scissors ? text.slice(0, scissors.index) : text
}

// Git strips comment lines (those starting with `#`) from the final message,
// so a mention on such a line is never committed.
function stripCommentLines (text) {
  return text
    .split('\n')
    .filter((line) => !line.trimStart().startsWith('#'))
    .join('\n')
}

// Replace each code span with a boundary char rather than removing it, so the
// text on either side cannot glue together and flip mention detection — e.g.
// `PR` + `` `x` `` + `@octocat` must still expose `@octocat`. Fenced blocks span
// lines (collapse to a newline); inline spans collapse to a space.
function stripCodeSpans (text) {
  return text
    .replace(/```[\s\S]*?```/g, '\n')
    .replace(/`[^`]*`/g, ' ')
}

// Collect every distinct `@handle` GitHub would linkify as a mention.
function findBareMentions (text) {
  const offenders = new Set()
  const handle = /@[a-z0-9](?:[a-z0-9-]*[a-z0-9])?(?:\/[a-z0-9._-]+)?/gi
  let match
  while ((match = handle.exec(text)) !== null) {
    if (isMentionBoundary(text, match.index)) {
      offenders.add(match[0])
    }
  }
  return offenders
}

// GitHub only linkifies `@name` when the `@` starts the text or follows a
// non-word char. This is what skips `user@example.com`, whose `@` follows a
// word char.
function isMentionBoundary (text, atIndex) {
  return atIndex === 0 || /\W/.test(text[atIndex - 1])
}

function reportAndExit (offenders) {
  const list = [...offenders].join(', ')
  console.error(`
✖ Commit message rejected: bare @mention(s) found: ${list}

A bare "@name" is ambiguous and frequently wrong. Wrap it in backticks
instead, or remove it.

WHY THIS IS BLOCKED
  GitHub turns any "@name" into a mention of that user/org/team. That is
  wrong in both of the ways "@name" is normally meant:

    1. If it is code (a scoped package like @pnpm/core, a handle, a path),
       GitHub should NOT treat it as a mention.

    2. If it really is a person, every push, force-push, and rebase that
       carries the commit re-notifies them — which is noise nobody asked for.

HOW TO FIX
  Wrap the reference in backticks so GitHub renders it as code and sends no
  notification:

      @pnpm/core   ->   \`@pnpm/core\`
      @foo         ->   \`@foo\`

  If you do not need the reference at all, just remove it.

DO NOT bypass this check with --no-verify, by editing/deleting this hook, or
with any suppression file. Fix the mention in the commit message instead.
`)
  process.exit(1)
}
