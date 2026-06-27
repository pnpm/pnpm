// Validates that a commit message header follows Conventional Commits, the same
// contract `@commitlint/config-conventional` enforced before husky and
// commitlint were removed. It has no npm dependencies so it runs in a fresh
// clone before `pnpm install`.
//
// Enforced on the header (`type(scope)!: subject`):
//   - type is one of the Conventional Commits types, lower-case
//   - subject is present, does not end with a period, and is not written like a
//     sentence (no leading capital) or in all caps
//   - the whole header is at most 100 characters
//
// Body/footer line-length limits are intentionally not enforced: long URLs and
// pasted output in bodies are common and CI is not the place this matters.

import { readFileSync } from 'node:fs'

const HEADER_MAX_LENGTH = 100

// Keep in sync with the type list from `@commitlint/config-conventional`.
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

// Auto-generated messages commitlint ignores by default — they never follow the
// Conventional Commits shape and must not be rejected.
const IGNORED_HEADER = [
  /^Merge /,
  /^Revert /,
  /^Revert "/,
  /^(fixup|squash)! /,
  /^Auto-merged /,
  /^Automatic merge/,
]

const msgPath = process.argv[2]
if (!msgPath) {
  console.error('check-conventional-commit: missing commit message file path argument')
  process.exit(1)
}

const header = readHeader(readFileSync(msgPath, 'utf8'))

if (header === '' || IGNORED_HEADER.some((re) => re.test(header))) {
  process.exit(0)
}

const errors = collectErrors(header)

if (errors.length === 0) {
  process.exit(0)
}

reportAndExit(header, errors)

// The header is the first non-empty, non-comment line. Git drops `#` comment
// lines from the final message, so they must not be treated as the header.
function readHeader (raw) {
  for (const line of raw.split('\n')) {
    if (line.trimStart().startsWith('#')) continue
    if (line.trim() === '') continue
    return line.replace(/\s+$/, '')
  }
  return ''
}

function collectErrors (header) {
  const errors = []

  if (header.length > HEADER_MAX_LENGTH) {
    errors.push(`header is ${header.length} characters; keep it within ${HEADER_MAX_LENGTH}`)
  }

  const match = /^([^():!]+)(?:\([^)]*\))?(?:!)?: (.*)$/.exec(header)
  if (!match) {
    errors.push('header must match "type(optional scope): subject", e.g. "fix(core): handle empty input"')
    return errors
  }

  const type = match[1]
  const subject = match[2]

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
  if (isUpperCase(subject)) {
    errors.push('subject must not be in all upper-case')
  } else if (isSentenceCase(subject)) {
    errors.push('subject must not start with a capital letter (do not write it like a sentence)')
  }

  return errors
}

function isUpperCase (text) {
  return text === text.toUpperCase() && text !== text.toLowerCase()
}

// "Sentence-case" as commitlint defines it: the string equals its all-lowercase
// form with only the first letter capitalized (e.g. "Add feature"). Mixed-case
// tokens like "API request" are not sentence-case and stay allowed.
function isSentenceCase (text) {
  const sentenceCased = text.charAt(0).toUpperCase() + text.slice(1).toLowerCase()
  return text === sentenceCased && /[A-Z]/.test(text.charAt(0))
}

function reportAndExit (header, errors) {
  console.error(`
✖ Commit message rejected — it does not follow Conventional Commits:

    ${header}

${errors.map((e) => `  • ${e}`).join('\n')}

A header looks like "type(optional scope): subject", for example:

    feat(config): add the blockExoticSubdeps setting
    fix: do not crash on an empty lockfile

Allowed types: ${[...ALLOWED_TYPES].join(', ')}.
`)
  process.exit(1)
}
