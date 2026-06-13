// Rejects commit messages that contain a bare `@name` mention.
//
// A "bare" mention is an `@` immediately followed by a username-like token
// that is NOT wrapped in backticks (inline code or a fenced code block) and
// is NOT part of an email address.
//
// Rationale lives in the error message below and in AGENTS.md.

import { readFileSync } from 'node:fs'

const msgPath = process.argv[2]
if (!msgPath) {
  console.error('reject-bare-mentions: missing commit message file path argument')
  process.exit(1)
}

const raw = readFileSync(msgPath, 'utf8')

// Drop git comment lines (those starting with `#`). Git strips them from the
// final message anyway, so a bare `@name` on such a line is never committed.
let message = raw
  .split('\n')
  .filter((line) => !line.trimStart().startsWith('#'))
  .join('\n')

// Strip backtick-wrapped spans before scanning: a mention inside a fenced code
// block or an inline code span is exactly the safe form we want people to use,
// so it must not be flagged. Remove fenced blocks first, then inline spans.
message = message
  .replace(/```[\s\S]*?```/g, '')
  .replace(/`[^`]*`/g, '')

// Match `@` + username where the char before `@` is not part of a word, email
// local part, or path. That skips `user@example.com` (preceded by a word char)
// while catching bare `@foo`, `(@foo)`, `thanks @foo`, and scoped package names
// like `@pnpm/core`.
const bareMentionRegExp = /(^|[^\w.@/-])@([a-z0-9][a-z0-9-]*(?:\/[a-z0-9._-]+)?)/gi

const offenders = new Set()
let match
while ((match = bareMentionRegExp.exec(message)) !== null) {
  offenders.add(`@${match[2]}`)
}

if (offenders.size === 0) {
  process.exit(0)
}

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
