import assert from 'node:assert/strict'
import console from 'node:console'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { test } from 'node:test'
import { checkedOutSource, formatStagedRust } from './format-staged-rust.mjs'
import { git, temporaryRepo } from './git-fixture.mjs'

// Names git hands back verbatim that a shell would have split or expanded, or
// that rustfmt would have read as an option. Windows has no file of the last
// kind, and `git add` reads a pathspec as a glob, so `star*.rs` is also the
// name that could stage `starX.rs` along with it.
const AWKWARD = process.platform === 'win32'
  ? ['-dash.rs', 'a b.rs']
  : ['-dash.rs', 'a b.rs', 'star*.rs']

function repoWithSources (context, files) {
  const repo = temporaryRepo(context, 'pnpm-format-staged-')
  // The rename case only arises where git detects renames. That is the
  // default, but it is a setting a temporary repository inherits.
  git(repo, 'config', 'diff.renames', 'true')
  // Enough lines that appending one leaves a renamed file similar enough for
  // git to report it as a rename rather than a delete and an add.
  const seed = Array.from({ length: 10 }, (item, index) => `fn original_${index}() {}\n`).join('')
  for (const name of files) fs.writeFileSync(path.join(repo, name), seed)
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

test('formats every staged Rust file and stages what changed', (context) => {
  const repo = repoWithSources(context, [...AWKWARD, 'kept.txt'])
  for (const name of [...AWKWARD, 'kept.txt']) fs.appendFileSync(path.join(repo, name), 'fn staged() {}\n')
  git(repo, 'add', '--all')

  const { seen, format } = reformatter()
  assert.equal(formatStagedRust(repo, { format }), 0)

  assert.deepEqual(seen.sort(), AWKWARD.map(name => path.join(repo, name)).sort())
  for (const name of AWKWARD) assert.match(git(repo, 'show', `:${name}`), /\/\/ reformatted/)
  assert.doesNotMatch(git(repo, 'show', ':kept.txt'), /\/\/ reformatted/)
})

test('formats the destination of a staged rename', (context) => {
  const repo = repoWithSources(context, ['before.rs'])
  git(repo, 'mv', 'before.rs', 'after.rs')
  fs.appendFileSync(path.join(repo, 'after.rs'), 'fn staged() {}\n')
  git(repo, 'add', '--all')
  assert.match(git(repo, 'diff', '--cached', '--name-status'), /^R/)

  const { seen, format } = reformatter()
  assert.equal(formatStagedRust(repo, { format }), 0)

  assert.deepEqual(seen, [path.join(repo, 'after.rs')])
  assert.match(git(repo, 'show', ':after.rs'), /\/\/ reformatted/)
})

test('leaves a staged file that has unstaged changes as well', (context) => {
  const repo = repoWithSources(context, ['held.rs', 'clean.rs'])
  for (const name of ['held.rs', 'clean.rs']) fs.appendFileSync(path.join(repo, name), 'fn staged() {}\n')
  git(repo, 'add', '--all')
  fs.appendFileSync(path.join(repo, 'held.rs'), 'fn withheld() {}\n')

  const { seen, format } = reformatter()
  assert.equal(formatStagedRust(repo, { format }), 0)

  assert.deepEqual(seen, [path.join(repo, 'clean.rs')])
  assert.doesNotMatch(git(repo, 'show', ':held.rs'), /withheld|reformatted/)
})

test('reports nothing to do when no Rust file is staged', (context) => {
  const repo = repoWithSources(context, ['untouched.rs'])
  fs.writeFileSync(path.join(repo, 'notes.txt'), 'text\n')
  git(repo, 'add', '--all')

  const { seen, format } = reformatter()
  assert.equal(formatStagedRust(repo, { format }), 0)
  assert.deepEqual(seen, [])
})

test('stages nothing when the formatter fails', (context) => {
  const repo = repoWithSources(context, ['broken.rs'])
  fs.appendFileSync(path.join(repo, 'broken.rs'), 'fn staged( {}\n')
  git(repo, 'add', '--all')
  const staged = git(repo, 'show', ':broken.rs')

  assert.equal(formatStagedRust(repo, { format: () => 1 }), 1)
  assert.equal(git(repo, 'show', ':broken.rs'), staged)
})

test('stages only the file the pathspec names literally', { skip: process.platform === 'win32' }, (context) => {
  const repo = repoWithSources(context, ['star*.rs', 'starX.rs'])
  fs.appendFileSync(path.join(repo, 'star*.rs'), 'fn staged() {}\n')
  git(repo, 'add', '--all')
  fs.appendFileSync(path.join(repo, 'starX.rs'), 'fn never_staged() {}\n')

  const { format } = reformatter()
  assert.equal(formatStagedRust(repo, { format }), 0)

  assert.match(git(repo, 'show', ':star*.rs'), /\/\/ reformatted/)
  assert.doesNotMatch(git(repo, 'show', ':starX.rs'), /never_staged/)
})

test('refuses to format a staged symlink', { skip: process.platform === 'win32' }, (context) => {
  const repo = repoWithSources(context, ['real.rs'])
  const outside = path.join(repo, '..', path.basename(repo) + '-outside.rs')
  fs.writeFileSync(outside, 'fn outside() {}\n')
  context.after(() => fs.rmSync(outside, { force: true }))
  fs.symlinkSync(outside, path.join(repo, 'escape.rs'))
  git(repo, 'add', '--all')

  const { seen, format } = reformatter()
  assert.equal(formatStagedRust(repo, { format }), 0)

  assert.deepEqual(seen, [])
  assert.equal(fs.readFileSync(outside, 'utf8'), 'fn outside() {}\n')
})

test('refuses a staged path whose directory became a link out of the checkout', { skip: process.platform === 'win32' }, (context) => {
  const repo = repoWithSources(context, ['real.rs'])
  fs.mkdirSync(path.join(repo, 'dir'))
  fs.writeFileSync(path.join(repo, 'dir', 'nested.rs'), 'fn nested() {}\n')
  git(repo, 'add', '--all')

  const outside = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-format-outside-'))
  context.after(() => fs.rmSync(outside, { recursive: true, force: true }))
  fs.writeFileSync(path.join(outside, 'nested.rs'), 'fn nested() {}\n')
  fs.rmSync(path.join(repo, 'dir'), { recursive: true })
  fs.symlinkSync(outside, path.join(repo, 'dir'))

  const reported = context.mock.method(console, 'error', () => {})
  const { seen, format } = reformatter()
  assert.equal(formatStagedRust(repo, { format }), 0)

  assert.deepEqual(seen, [])
  assert.match(reported.mock.calls[0].arguments[0], /each must name one regular file inside the checkout:\n {2}dir\/nested\.rs/)
  assert.equal(fs.readFileSync(path.join(outside, 'nested.rs'), 'utf8'), 'fn nested() {}\n')
})

test('rejects a source path that resolves outside the checkout', { skip: process.platform === 'win32' }, (context) => {
  const repo = temporaryRepo(context, 'pnpm-format-contain-')
  fs.writeFileSync(path.join(repo, 'inside.rs'), 'fn inside() {}\n')
  const outside = `${repo}-outside.rs`
  fs.writeFileSync(outside, 'fn outside() {}\n')
  context.after(() => fs.rmSync(outside, { force: true }))

  fs.mkdirSync(path.join(repo, 'real-dir'))
  fs.writeFileSync(path.join(repo, 'real-dir', 'nested.rs'), 'fn nested() {}\n')
  fs.symlinkSync(path.join(repo, 'real-dir'), path.join(repo, 'link-dir'))

  assert.equal(checkedOutSource(repo, 'inside.rs'), path.join(repo, 'inside.rs'))
  // The path the formatter is handed is the resolved one, not the one that
  // still has a link to walk.
  assert.equal(checkedOutSource(repo, 'link-dir/nested.rs'), path.join(repo, 'real-dir', 'nested.rs'))
  assert.equal(checkedOutSource(repo, `../${path.basename(outside)}`), null)
  assert.equal(checkedOutSource(repo, 'missing.rs'), null)
})

test('refuses to format a staged hardlink to a file outside the checkout', { skip: process.platform === 'win32' }, (context) => {
  const repo = repoWithSources(context, ['real.rs'])
  const outside = `${repo}-outside.rs`
  fs.writeFileSync(outside, 'fn outside() {}\n')
  context.after(() => fs.rmSync(outside, { force: true }))
  fs.linkSync(outside, path.join(repo, 'linked.rs'))
  git(repo, 'add', '--all')

  const reported = context.mock.method(console, 'error', () => {})
  const { seen, format } = reformatter()
  assert.equal(formatStagedRust(repo, { format }), 0)

  assert.deepEqual(seen, [])
  assert.match(reported.mock.calls[0].arguments[0], /must name one regular file inside the checkout:\n {2}linked\.rs/)
  assert.equal(fs.readFileSync(outside, 'utf8'), 'fn outside() {}\n')
})

// Each replacement makes `dir/nested.rs` fail to resolve with a different
// errno, and every one of them used to abort the commit.
for (const [errno, replaceDirectory] of [
  ['ENOTDIR', (dir) => fs.writeFileSync(dir, 'no longer a directory\n')],
  ['ELOOP', (dir) => fs.symlinkSync(path.basename(dir), dir)],
]) {
  test(`reports a staged path that stops resolving with ${errno}`, { skip: process.platform === 'win32' }, (context) => {
    const repo = repoWithSources(context, ['real.rs'])
    fs.mkdirSync(path.join(repo, 'dir'))
    fs.writeFileSync(path.join(repo, 'dir', 'nested.rs'), 'fn nested() {}\n')
    git(repo, 'add', '--all')
    fs.rmSync(path.join(repo, 'dir'), { recursive: true })
    replaceDirectory(path.join(repo, 'dir'))
    assert.throws(() => fs.lstatSync(path.join(repo, 'dir', 'nested.rs')), { code: errno })

    const reported = context.mock.method(console, 'error', () => {})
    const { seen, format } = reformatter()
    assert.equal(formatStagedRust(repo, { format }), 0)

    assert.deepEqual(seen, [])
    assert.match(reported.mock.calls[0].arguments[0], /must name one regular file inside the checkout:\n {2}dir\/nested\.rs/)
  })
}
