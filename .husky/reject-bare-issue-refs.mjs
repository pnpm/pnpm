// Rejects commit messages that contain a bare `#NNN` issue/PR reference.
//
// A "bare" reference is a `#` immediately followed by digits that is NOT
// qualified by an `owner/repo` prefix (e.g. `pnpm/pnpm#123`) and is NOT part
// of an absolute URL.
//
// Rationale lives in the error message below and in CLAUDE.md.

import { readFileSync } from 'node:fs'

const msgPath = process.argv[2]
if (!msgPath) {
  console.error('reject-bare-issue-refs: missing commit message file path argument')
  process.exit(1)
}

const raw = readFileSync(msgPath, 'utf8')

// Drop git comment lines (those starting with `#`). Git strips them from the
// final message anyway, so a bare `#NNN` on such a line is never committed.
const message = raw
  .split('\n')
  .filter((line) => !line.trimStart().startsWith('#'))
  .join('\n')

// Match `#` + digits where the char before `#` is not part of a repo slug or
// URL path. That keeps `pnpm/pnpm#123` and `github.com/pnpm/pnpm/...` allowed
// while catching bare `#123`, `(#123)`, `fixes #123`, etc.
const bareRefRegExp = /(^|[^\w./-])#(\d+)/g

const offenders = new Set()
let match
while ((match = bareRefRegExp.exec(message)) !== null) {
  offenders.add(`#${match[2]}`)
}

if (offenders.size === 0) {
  process.exit(0)
}

const list = [...offenders].join(', ')

console.error(`
✖ Commit message rejected: bare issue/PR reference(s) found: ${list}

A bare "#NNN" reference is ambiguous and frequently wrong. Use one of the
unambiguous forms instead, or remove the reference.

WHY THIS IS BLOCKED
  GitHub silently turns any "#NNN" into a link to issue/PR NNN of THIS repo.
  That makes "#NNN" dangerous in two common ways:

    1. It is sometimes meant as a list item ("#1", "#2", "#3" pointing at
       numbered items in the body). GitHub will instead link it to unrelated
       issues #1, #2, #3 of this repo.

    2. It is sometimes meant as issue NNN of a DIFFERENT repository. GitHub
       will instead link it to issue NNN of this repo — a completely
       different ticket.

HOW TO FIX (address the root cause — see which case you are in)

  • Referencing an issue/PR in THIS repo?
      Qualify it:        pnpm/pnpm#NNN
      or use a URL:      https://github.com/pnpm/pnpm/issues/NNN

  • Referencing an issue/PR in ANOTHER repo?
      Qualify it:        owner/repo#NNN
      or use a URL:      https://github.com/owner/repo/issues/NNN

  • Enumerating list items (1st, 2nd, 3rd ...)?
      Don't use "#". Write "item 1", "(1)", "1." or rephrase so the number
      cannot be read as an issue reference.

Qualified syntax and absolute URLs are always safe — even for this repo — so
this rule is applied to every "#NNN". Always prefer them.

DO NOT bypass this check with --no-verify, by editing/deleting this hook, or
with any suppression file. Fix the reference in the commit message instead.
`)

process.exit(1)
