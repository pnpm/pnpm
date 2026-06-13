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

// `git commit -v` appends the diff below a scissors line; git discards
// everything from that line onward, so the hook must not scan it either —
// otherwise an `@mention` that only appears in the diff (e.g. a scoped-package
// import) would wrongly reject a commit, on text the author never wrote in the
// message and cannot remove.
const scissors = raw.match(/^#\s*-{2,}\s*>8\s*-{2,}.*$/m)
const committable = scissors ? raw.slice(0, scissors.index) : raw

// Drop git comment lines (those starting with `#`). Git strips them from the
// final message anyway, so a bare `@name` on such a line is never committed.
let message = committable
  .split('\n')
  .filter((line) => !line.trimStart().startsWith('#'))
  .join('\n')

// Replace backtick-wrapped spans before scanning: a mention inside a fenced
// code block or an inline code span is exactly the safe form we want people to
// use, so it must not be flagged. Each span is replaced with a boundary char
// (not removed) so the text on either side cannot glue together and flip the
// mention-boundary classification — e.g. `PR` + `` `x` `` + `@octocat` must
// still expose `@octocat` as a mention, the way GitHub renders it. Fenced
// blocks (which span lines) collapse to a newline; inline spans to a space.
message = message
  .replace(/```[\s\S]*?```/g, '\n')
  .replace(/`[^`]*`/g, ' ')

// Match `@` + username at a mention boundary, mirroring how GitHub linkifies
// mentions: the `@` must be at the start of input or follow a non-word char.
// An email like `user@example.com` is skipped because its `@` follows a word
// char, while `@foo`, `.@foo`, `(@foo)`, `thanks @foo`, and scoped names like
// `@pnpm/core` are caught. The username forbids a trailing hyphen so the
// reported handle matches what GitHub would actually link.
const bareMentionRegExp = /(^|[^\w])@([a-z0-9](?:[a-z0-9-]*[a-z0-9])?(?:\/[a-z0-9._-]+)?)/gi

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
