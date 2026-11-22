import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { test } from 'node:test'
import { URL } from 'node:url'
import { determineTestScope } from './ts-test-scope.mjs'

test('PR scope stays pinned when main advances; global changes and missing bases run everything', () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'ci-scope-'))
  const origin = path.join(dir, 'origin')
  const checkout = path.join(dir, 'checkout')
  const git = (cwd, ...args) => execFileSync('git', args, { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim()
  const commit = (cwd, file, content) => {
    mkdirSync(path.dirname(path.join(cwd, file)), { recursive: true })
    writeFileSync(path.join(cwd, file), content)
    git(cwd, 'add', '.')
    git(cwd, '-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', '-c', 'commit.gpgsign=false', 'commit', '-m', 'test')
    return git(cwd, 'rev-parse', 'HEAD')
  }
  try {
    mkdirSync(origin)
    git(origin, 'init', '-b', 'main')
    commit(origin, 'pnpm11/example/src/index.ts', 'base')
    const base = commit(origin, 'global-test-config.json', 'original config')
    git(dir, 'clone', origin, checkout)
    git(checkout, 'switch', '-c', 'pr')
    commit(checkout, 'pnpm11/example/src/index.ts', 'pr')
    commit(origin, 'pnpm11/unrelated/src/index.ts', 'new main')
    const scope = () => determineTestScope({ event: 'pull_request', base, cwd: checkout })
    assert.equal(scope().full_tests, 'false')
    assert.equal(git(checkout, 'rev-parse', 'origin/main'), base)
    assert.equal(git(checkout, 'diff', '--name-only', 'origin/main', 'HEAD'), 'pnpm11/example/src/index.ts')
    commit(checkout, '.changeset/fix-example.md', 'Release note')
    assert.equal(scope().full_tests, 'false', 'TS code plus release notes remain affected')
    for (const file of ['package.json', 'pnpm-lock.yaml', '.config/pnpm/pnpmfile.cjs', 'pnpm-workspace.yaml', '.config/pnpm/patches/dependency.patch', '.github/scripts/new-helper.mjs', '.changeset/config.json', 'pnpr/client/src/index.ts', 'pnpr/.fixtures/packages/example/package.json']) {
      commit(checkout, file, 'global input')
      assert.equal(scope().full_tests, 'true', file)
      git(checkout, 'reset', '--hard', 'HEAD^')
    }
    git(checkout, 'mv', 'global-test-config.json', 'pnpm11/example/moved-config.json')
    git(checkout, '-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', '-c', 'commit.gpgsign=false', 'commit', '-m', 'rename global config')
    assert.equal(scope().full_tests, 'true', 'moving a global input into a package still affects all tests')
    git(checkout, 'reset', '--hard', 'HEAD^')
    git(checkout, 'rm', 'global-test-config.json')
    git(checkout, '-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', '-c', 'commit.gpgsign=false', 'commit', '-m', 'delete global config')
    assert.equal(scope().full_tests, 'true', 'deleted global inputs still affect all tests')
    assert.equal(determineTestScope({ event: 'pull_request', base: 'f'.repeat(40), cwd: checkout }).full_tests, 'true')
  } finally {
    rmSync(dir, { recursive: true, force: true })
  }
})

test('merge queues, main, manual and unknown events cannot narrow tests', () => {
  for (const event of ['merge_group', 'push', 'workflow_dispatch', 'unknown', undefined]) {
    assert.equal(determineTestScope({ event, base: 'a'.repeat(40) }).full_tests, 'true')
  }
  for (const base of [undefined, '', 'main', '--upload-pack=evil', 'a'.repeat(39)]) {
    assert.equal(determineTestScope({ event: 'pull_request', base }).full_tests, 'true')
  }
})

test('TS gate includes the registry sources and fixtures used by the v11 tests', () => {
  const workflow = readFileSync(new URL('../workflows/ci.yml', import.meta.url), 'utf8')
  const filters = workflow.split('            ts:\n')[1].split('            changesets:\n')[0]
  const patterns = filters.split('\n').map(line => line.match(/- '([^']+)'/)?.[1]).filter(Boolean)
  for (const pattern of ['pnpm11/**', 'pnpr/**', '.github/workflows/build-pnpr.yml']) {
    assert.ok(patterns.includes(pattern), `required TS test input: ${pattern}`)
  }
})
