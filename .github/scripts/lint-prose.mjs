#!/usr/bin/env node
// Runs Vale over the tracked prose: Markdown, and the comments in Rust and
// TypeScript sources. Extra arguments are passed to Vale, so
// `--minAlertLevel=warning` also shows the rules that do not fail the run.
//
// Released changesets stay in .changeset/ and are not rewritten, so only the
// changesets added since PROSE_BASE (default: origin/main) are linted.
import { spawnSync } from 'node:child_process'
import console from 'node:console'
import process from 'node:process'

const PATTERNS = ['*.md', '*.rs', '*.ts']
const EXCLUDED = /(^|\/)(node_modules|vendor|fixtures|__fixtures__|\.fixtures|test-fixtures)\/|(^|\/)(CHANGELOG|LICENSE)\.md$|^\.changeset\//
// Stays under the Windows command-line limit.
const FILES_PER_RUN = 200

const base = process.env.PROSE_BASE ?? 'origin/main'
const files = [
  ...git(['ls-files', '-z', '--', ...PATTERNS]).filter((file) => !EXCLUDED.test(file)),
  ...listAddedChangesets(base),
]
const valeArgs = ['--minAlertLevel=error', ...process.argv.slice(2)]
let failed = false
for (let start = 0; start < files.length; start += FILES_PER_RUN) {
  const result = spawnSync('vale', [...valeArgs, ...files.slice(start, start + FILES_PER_RUN)], { stdio: 'inherit' })
  if (result.error != null) {
    console.error(`Could not run vale: ${result.error.message}. Install it from https://vale.sh/docs/install.`)
    process.exit(1)
  }
  failed ||= result.status !== 0
}
process.exit(failed ? 1 : 0)

function listAddedChangesets (rev) {
  const added = git(['diff', '-z', '--name-only', '--diff-filter=A', rev, '--', '.changeset/*.md'])
  const untracked = git(['ls-files', '-z', '--others', '--exclude-standard', '--', '.changeset/*.md'])
  return [...added, ...untracked].filter((file) => !file.startsWith('.changeset/changelogs/'))
}

function git (args) {
  const result = spawnSync('git', args, { encoding: 'utf8' })
  if (result.status !== 0) {
    throw new Error(`git ${args.join(' ')} failed: ${result.stderr}`)
  }
  return result.stdout.split('\0').filter(Boolean)
}
