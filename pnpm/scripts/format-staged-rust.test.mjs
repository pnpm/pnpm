import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { test } from 'node:test'
import { formatStagedRust, partitionStaged } from './format-staged-rust.mjs'

// Names git hands back verbatim that a shell would have split, expanded, or
// handed to rustfmt as an option. Windows has no file of the last kind.
const AWKWARD = process.platform === 'win32'
  ? ['-dash.rs', 'a b.rs']
  : ['-dash.rs', 'a b.rs', 'star*.rs']

function git (repo, ...args) {
  const result = spawnSync('git', args, {
    cwd: repo,
    encoding: 'utf8',
    env: { ...process.env, GIT_AUTHOR_NAME: 't', GIT_AUTHOR_EMAIL: 't@t', GIT_COMMITTER_NAME: 't', GIT_COMMITTER_EMAIL: 't@t' },
  })
  assert.equal(result.status, 0, `git ${args.join(' ')} failed: ${result.stderr}`)
  return result.stdout
}

function temporaryRepo (context, files) {
  const repo = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-format-staged-')))
  context.after(() => fs.rmSync(repo, { recursive: true, force: true }))
  git(repo, 'init')
  for (const name of files) fs.writeFileSync(path.join(repo, name), 'fn original() {}\n')
  git(repo, 'add', '--all')
  git(repo, 'commit', '-m', 'root', '--no-verify')
  return repo
}

function reformatter () {
  const seen = []
  return {
    seen,
    format (files) {
      seen.push(...files)
      for (const file of files) fs.appendFileSync(file, '// reformatted\n')
      return 0
    },
  }
}

test('splits the staged files by whether the working tree holds more', () => {
  assert.deepEqual(partitionStaged(['a.rs', 'b.rs', 'c.rs'], ['b.rs', 'd.rs']), {
    formattable: ['a.rs', 'c.rs'],
    withheld: ['b.rs'],
  })
})

test('formats every staged Rust file and stages what changed', (context) => {
  const repo = temporaryRepo(context, [...AWKWARD, 'kept.txt'])
  for (const name of [...AWKWARD, 'kept.txt']) fs.appendFileSync(path.join(repo, name), 'fn staged() {}\n')
  git(repo, 'add', '--all')

  const { seen, format } = reformatter()
  assert.equal(formatStagedRust(repo, { format }), 0)

  assert.deepEqual(seen.sort(), AWKWARD.map(name => path.join(repo, name)).sort())
  for (const name of AWKWARD) assert.match(git(repo, 'show', `:${name}`), /\/\/ reformatted/)
  assert.doesNotMatch(git(repo, 'show', ':kept.txt'), /\/\/ reformatted/)
})

test('leaves a staged file that has unstaged changes as well', (context) => {
  const repo = temporaryRepo(context, ['held.rs', 'clean.rs'])
  for (const name of ['held.rs', 'clean.rs']) fs.appendFileSync(path.join(repo, name), 'fn staged() {}\n')
  git(repo, 'add', '--all')
  fs.appendFileSync(path.join(repo, 'held.rs'), 'fn withheld() {}\n')

  const { seen, format } = reformatter()
  assert.equal(formatStagedRust(repo, { format }), 0)

  assert.deepEqual(seen, [path.join(repo, 'clean.rs')])
  assert.doesNotMatch(git(repo, 'show', ':held.rs'), /withheld|reformatted/)
})

test('reports nothing to do when no Rust file is staged', (context) => {
  const repo = temporaryRepo(context, ['untouched.rs'])
  fs.writeFileSync(path.join(repo, 'notes.txt'), 'text\n')
  git(repo, 'add', '--all')

  const { seen, format } = reformatter()
  assert.equal(formatStagedRust(repo, { format }), 0)
  assert.deepEqual(seen, [])
})

test('stages nothing when the formatter fails', (context) => {
  const repo = temporaryRepo(context, ['broken.rs'])
  fs.appendFileSync(path.join(repo, 'broken.rs'), 'fn staged( {}\n')
  git(repo, 'add', '--all')
  const staged = git(repo, 'show', ':broken.rs')

  assert.equal(formatStagedRust(repo, { format: () => 1 }), 1)
  assert.equal(git(repo, 'show', ':broken.rs'), staged)
})
