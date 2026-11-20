// Validates that a commit message header follows Conventional Commits, with the
// header rules of `@commitlint/config-conventional`. It has no npm dependencies
// so it runs in a fresh clone before `pnpm install`.
//
// Enforced on the header (`type(scope)!: subject`):
//   - type is one of the Conventional Commits types, lower-case
//   - subject is present, does not end with a period, does not start with a
//     capital letter, and is not in all caps
//   - the header has no surrounding whitespace and is at most 100 characters
//
// Body/footer line-length limits are intentionally not enforced: long URLs and
// pasted output in bodies are common.

import { readFileSync } from 'node:fs'
import path from 'node:path'
import process from 'node:process'
import { pathToFileURL } from 'node:url'

const HEADER_MAX_LENGTH = 100

const ALLOWED_TYPES = new Set([
  'build',
  'chore',
  'ci',
  'docs',
  'feat',
  'fix',
  'perf',
  'refactor',
  'revert',
  'style',
  'test',
])

// Auto-generated headers that never follow the Conventional Commits shape.
const IGNORED_HEADER = [
  /^Merge /,
  /^Merged /,
  /^[Rr]evert /,
  /^[Rr]eapply /,
  /^(?:amend|fixup|squash)!/,
  /^Auto-merged /,
  /^Automatic merge/,
]

/**
 * Returns the reasons the commit message's header breaks the rules, or an
 * empty array when it is valid or is an auto-generated header that is exempt.
 */
export function lintCommitMessage (raw) {
  const header = readHeader(raw)
  if (header.trim() === '' || IGNORED_HEADER.some((re) => re.test(header))) {
    return []
  }
  return collectErrors(header)
}

// The header is the first non-empty, non-comment line. Git drops `#` comment
// lines from the final message, so they must not be treated as the header.
function readHeader (raw) {
  for (const line of raw.split(/\r?\n/)) {
    if (line.startsWith('#') || line.trim() === '') continue
    return line
  }
  return ''
}

function collectErrors (header) {
  const errors = []

  if (header !== header.trim()) {
    errors.push('header must not start or end with whitespace')
  }
  if (header.length > HEADER_MAX_LENGTH) {
    errors.push(`header is ${header.length} characters; keep it within ${HEADER_MAX_LENGTH}`)
  }

  const match = /^([^():!\s]+)(?:\([^)]*\))?!?:(?: (.*))?$/.exec(header.trim())
  if (!match) {
    errors.push('header must match "type(optional scope): subject", e.g. "fix(core): handle empty input"')
    return errors
  }

  const [, type, subject = ''] = match

  if (type !== type.toLowerCase()) {
    errors.push(`type "${type}" must be lower-case`)
  }
  if (!ALLOWED_TYPES.has(type.toLowerCase())) {
    errors.push(`type "${type}" is not allowed; use one of: ${[...ALLOWED_TYPES].join(', ')}`)
  }

  if (subject.trim() === '') {
    errors.push('subject must not be empty')
    return errors
  }
  if (subject.endsWith('.')) {
    errors.push('subject must not end with a period')
  }

  // Quoted text may carry proper names (e.g. "`Eslint` config"), so it is
  // ignored by the case checks, as commitlint does.
  const casedText = subject.replace(/`.*?`|".*?"|'.*?'/g, '').trim()
  // Scripts without letter case, such as Chinese, have nothing to check.
  const hasCasedLetter = casedText.toUpperCase() !== casedText.toLowerCase()
  if (hasCasedLetter && !/^\d/.test(casedText)) {
    if (casedText === casedText.toUpperCase()) {
      errors.push('subject must not be in all upper-case')
    } else if (casedText.charAt(0) !== casedText.charAt(0).toLowerCase()) {
      errors.push('subject must not start with a capital letter')
    }
  }

  return errors
}

function formatReport (header, errors) {
  return `
✖ Commit message rejected — it does not follow Conventional Commits:

    ${header}

${errors.map((e) => `  • ${e}`).join('\n')}

A header looks like "type(optional scope): subject", for example:

    feat(config): add the blockExoticSubdeps setting
    fix: do not crash on an empty lockfile

Allowed types: ${[...ALLOWED_TYPES].join(', ')}.
`
}

if (process.argv[1] && pathToFileURL(path.resolve(process.argv[1])).href === import.meta.url) {
  const msgPath = process.argv[2]
  if (!msgPath) {
    console.error('check-conventional-commit: missing commit message file path argument')
    process.exit(1)
  }
  const raw = readFileSync(msgPath, 'utf8')
  const errors = lintCommitMessage(raw)
  if (errors.length > 0) {
    console.error(formatReport(readHeader(raw), errors))
    process.exit(1)
  }
}
