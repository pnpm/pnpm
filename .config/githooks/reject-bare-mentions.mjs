// Rejects commit messages that contain a bare `@name` mention — one that is not
// wrapped in backticks. GitHub turns such a token into a real notification.
//
// Rationale lives in the error message below and in AGENTS.md.

import { readFileSync } from 'node:fs'

const messagePath = process.argv[2]
if (!messagePath) {
  console.error('reject-bare-mentions: missing commit message file path argument')
  process.exit(1)
}

const offenders = findBareMentions(scannableText(readFileSync(messagePath, 'utf8')))

if (offenders.size === 0) {
  process.exit(0)
}

reportAndExit(offenders)

// Reduce the raw commit-message file to the text git will actually keep: the
// diff that `git commit -v` appends below the scissors line is dropped, and
// `#` comment lines are dropped — git strips both, so a mention in either is
// never committed and must not be flagged.
function scannableText (raw) {
  return stripCommentLines(stripScissorsSection(raw))
}

function stripScissorsSection (raw) {
  const lines = raw.split('\n')
  const cut = lines.findIndex(isScissorsLine)
  return cut === -1 ? raw : lines.slice(0, cut).join('\n')
}

// git's `commit -v` cut line, e.g. "# ------------------------ >8 ------------------------".
function isScissorsLine (line) {
  return line.startsWith('#') && line.includes('>8') && line.includes('--')
}

function stripCommentLines (text) {
  return text
    .split('\n')
    .filter((line) => !line.trimStart().startsWith('#'))
    .join('\n')
}

// Find every distinct `@handle` that GitHub would linkify as a mention. We only
// care about GitHub's rendering, not the exact username rules: an `@` is a
// mention when it is not inside code, is followed by an ASCII letter or digit,
// and is not preceded by one (which would make it part of an email address).
function findBareMentions (text) {
  const offenders = new Set()
  for (let i = 0; i < text.length; i++) {
    if (text[i] !== '@') continue
    if (isInsideBackticks(text, i)) continue
    if (!isAsciiAlphaNumeric(text[i + 1])) continue
    if (isAsciiAlphaNumeric(text[i - 1])) continue
    offenders.add(readHandle(text, i))
  }
  return offenders
}

// An `@` sits inside a code span when an odd number of backticks precede it (one
// is still open). This covers inline `` `code` `` and triple-backtick fences
// alike, without parsing Markdown.
function isInsideBackticks (text, index) {
  let backticks = 0
  for (let i = 0; i < index; i++) {
    if (text[i] === '`') backticks++
  }
  return backticks % 2 === 1
}

// Read the handle starting at the `@`, for display in the error message. The
// caller only invokes this once the char after `@` is an ASCII alphanumeric, so
// that first char is always kept; trailing punctuation is then trimmed so the
// reported token is the part GitHub would actually link (e.g. the sentence-ending
// dot in "@pnpm/core." is dropped).
function readHandle (text, atIndex) {
  let end = atIndex + 1
  while (isHandleCharacter(text[end])) end++
  while (end > atIndex + 1 && !isAsciiAlphaNumeric(text[end - 1])) end--
  return text.slice(atIndex, end)
}

function isHandleCharacter (character) {
  return isAsciiAlphaNumeric(character) ||
    character === '-' ||
    character === '_' ||
    character === '.' ||
    character === '/'
}

function isAsciiAlphaNumeric (character) {
  return character !== undefined && (
    (character >= 'a' && character <= 'z') ||
    (character >= 'A' && character <= 'Z') ||
    (character >= '0' && character <= '9')
  )
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
