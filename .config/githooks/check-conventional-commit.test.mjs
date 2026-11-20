import assert from 'node:assert/strict'
import { test } from 'node:test'
import { lintCommitMessage } from './check-conventional-commit.mjs'

test('accepts conventional headers', () => {
  for (const header of [
    'feat: add a setting',
    'fix(core): handle empty input',
    'refactor(lockfile)!: drop the v5 format',
    'chore: bump `Eslint` to the latest',
    'docs: mention "Node.js" in the guide',
    'perf: 2x faster resolution',
    'test: cover iOS paths',
    'fix: 修复安装失败',
    'fix(core): 修复 lockfile 解析',
  ]) {
    assert.deepEqual(lintCommitMessage(`${header}\n\nBody.\n`), [], header)
  }
})

test('skips comment lines and leading blank lines when finding the header', () => {
  assert.deepEqual(lintCommitMessage('# Please enter the commit message\n\nfix: a bug\n'), [])
})

test('ignores auto-generated headers', () => {
  for (const header of [
    'Merge branch \'main\' into topic',
    'Revert "feat: add a setting"',
    'fixup! feat: add a setting',
    'amend! feat: add a setting',
    'squash! feat: add a setting',
  ]) {
    assert.deepEqual(lintCommitMessage(header), [], header)
  }
})

test('accepts an empty message, which git aborts on its own', () => {
  assert.deepEqual(lintCommitMessage('# only a comment\n'), [])
})

test('rejects a header without a type', () => {
  assert.equal(lintCommitMessage('add a setting').length, 1)
})

test('rejects an unknown or upper-case type', () => {
  assert.match(lintCommitMessage('feature: add a setting').join('\n'), /not allowed/)
  assert.match(lintCommitMessage('Feat: add a setting').join('\n'), /must be lower-case/)
})

test('rejects a subject that starts with a capital letter', () => {
  assert.match(lintCommitMessage('feat: Add a setting').join('\n'), /capital letter/)
  assert.match(lintCommitMessage('feat: Add API support').join('\n'), /capital letter/)
  assert.match(lintCommitMessage('feat: Add New Setting').join('\n'), /capital letter/)
})

test('rejects an all upper-case subject', () => {
  assert.match(lintCommitMessage('feat: ADD A SETTING').join('\n'), /all upper-case/)
})

test('rejects a subject ending with a period', () => {
  assert.match(lintCommitMessage('fix: handle empty input.').join('\n'), /period/)
})

test('rejects an empty subject', () => {
  assert.match(lintCommitMessage('fix: ').join('\n'), /must not be empty/)
})

test('rejects a header with surrounding whitespace', () => {
  assert.match(lintCommitMessage(' fix: handle empty input').join('\n'), /whitespace/)
  assert.match(lintCommitMessage('fix: handle empty input ').join('\n'), /whitespace/)
})

test('rejects a header longer than 100 characters', () => {
  assert.match(lintCommitMessage(`fix: ${'a'.repeat(96)}`).join('\n'), /101 characters/)
  assert.deepEqual(lintCommitMessage(`fix: ${'a'.repeat(95)}`), [])
})
