import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { test } from 'node:test'
import { prepareDocsSync } from './prepare-docs-sync.mjs'

function fixture (t, tag = 'v12.8.2') {
  const repo = mkdtempSync(path.join(os.tmpdir(), 'pnpm-docs-release-'))
  t.after(() => rmSync(repo, { recursive: true, force: true, maxRetries: 3 }))
  const git = (...args) => execFileSync('git', args, { cwd: repo, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim()
  const version = tag.startsWith('pnpr@') ? tag.slice(5) : tag.slice(1)
  for (const [product, current] of [['pnpm/npm/pnpm', '12.8.2'], ['pnpm11/pnpm', '11.28.2'], ['pnpr/npm/pnpr', '0.1.0-alpha.14']]) {
    mkdirSync(path.join(repo, product), { recursive: true })
    const productVersion = version.split('.')[0] === current.split('.')[0] ? version : current
    writeFileSync(path.join(repo, product, 'package.json'), JSON.stringify({ version: productVersion }))
  }
  git('init', '--quiet')
  git('config', 'commit.gpgsign', 'false')
  git('config', 'user.name', 'Test')
  git('config', 'user.email', 'test@example.com')
  git('add', '.')
  git('commit', '--quiet', '-m', 'chore: release')
  git('update-ref', `refs/tags/${tag}`, 'HEAD')
  const sha = git('rev-parse', 'HEAD')
  let verified = false
  const options = {
    releaseTag: tag,
    git: (...args) => {
      if (args[0] === 'verify-tag') { verified = true; return '' }
      return git(...args)
    },
    publicationState: spec => {
      assert.equal(verified, true)
      assert.equal(spec, `${tag.startsWith('pnpr@') ? '@pnpm/pnpr' : 'pnpm'}@${version}`)
      return 'published'
    },
  }
  return { repo, git, sha, options }
}

test('selects only the signed and published release snapshot', t => {
  const f = fixture(t)
  assert.deepEqual(prepareDocsSync(f.options), {
    publish: true, line: '12.x', version: '12.8.2', release_commit: f.sha, docs_commit: f.sha,
  })
  assert.throws(() => prepareDocsSync({ ...f.options, publicationState: () => 'missing' }), /not been published/)
  assert.throws(() => prepareDocsSync({ ...f.options, git: (...args) => args[0] === 'verify-tag' ? f.git(...args) : f.options.git(...args) }))
})

test('ignores prereleases and rejects releases outside stable v11 and v12', () => {
  assert.throws(() => prepareDocsSync({ releaseTag: 'pnpm-wasm-v1.0.0' }), /Expected a pnpm or pnpr release tag/)
  assert.equal(prepareDocsSync({ releaseTag: 'v12.9.0-beta.1' }), undefined)
  for (const tag of ['v10.30.0', 'v13.0.0']) {
    assert.throws(() => prepareDocsSync({ releaseTag: tag }), /v11 and v12/)
  }
})

test('manual corrections must contain only the selected version docs', t => {
  const f = fixture(t)
  mkdirSync(path.join(f.repo, 'pnpm/docs'), { recursive: true })
  writeFileSync(path.join(f.repo, 'pnpm/docs/install.md'), 'correction')
  f.git('add', '.')
  f.git('commit', '--quiet', '-m', 'docs: correction')
  const options = { ...f.options, releaseTag: 'v12.8.2', docsRef: f.git('rev-parse', 'HEAD') }
  assert.equal(prepareDocsSync(options).docs_commit, options.docsRef)
  assert.throws(() => prepareDocsSync({ ...options, docsRef: 'main' }), /full commit SHA/)
  writeFileSync(path.join(f.repo, 'unreleased.js'), 'new behavior')
  f.git('add', '.')
  f.git('commit', '--quiet', '-m', 'feat: unreleased behavior')
  assert.throws(() => prepareDocsSync({ ...options, docsRef: f.git('rev-parse', 'HEAD') }), /change only/)
})


test('v11 corrections use the TypeScript product documentation', t => {
  const f = fixture(t, 'v11.28.3')
  mkdirSync(path.join(f.repo, 'pnpm11/docs'), { recursive: true })
  writeFileSync(path.join(f.repo, 'pnpm11/docs/install.md'), 'correction')
  f.git('add', '.')
  f.git('commit', '--quiet', '-m', 'docs: v11 correction')
  const result = prepareDocsSync({ ...f.options, releaseTag: 'v11.28.3', docsRef: f.git('rev-parse', 'HEAD') })
  assert.equal(result.line, '11.x')
})


test('pnpr alpha releases and corrections select only the registry documentation', t => {
  const f = fixture(t, 'pnpr@0.1.0-alpha.15')
  assert.equal(prepareDocsSync(f.options).line, 'pnpr')
  mkdirSync(path.join(f.repo, 'pnpr/docs'), { recursive: true })
  writeFileSync(path.join(f.repo, 'pnpr/docs/installation.md'), 'correction')
  f.git('add', '.')
  f.git('commit', '--quiet', '-m', 'docs: registry correction')
  const result = prepareDocsSync({ ...f.options, releaseTag: 'pnpr@0.1.0-alpha.15', docsRef: f.git('rev-parse', 'HEAD') })
  assert.equal(result.line, 'pnpr')
  assert.equal(result.docs_commit, f.git('rev-parse', 'HEAD'))
})

test('manual publication from main needs no release or npm publication', t => {
  const f = fixture(t)
  assert.deepEqual(prepareDocsSync({
    githubRef: 'refs/heads/main',
    git: f.git,
    publicationState: () => assert.fail('main publication must not query npm'),
  }), { publish: true, main_sync: true, docs_commit: f.sha })
})

test('manual publication without a release is restricted to main and its checked-out snapshot', () => {
  for (const githubRef of [undefined, 'refs/heads/feature', 'refs/tags/v12.8.2']) {
    assert.throws(() => prepareDocsSync({ githubRef }), /must run from main/)
  }
  assert.throws(() => prepareDocsSync({
    githubRef: 'refs/heads/main', docsRef: 'a'.repeat(40),
  }), /docs_commit requires a release_tag/)
})

test('a release event only dispatches the sync from the default branch', () => {
  const workflow = readFileSync(new URL('../workflows/sync-docs.yml', import.meta.url), 'utf8')
  const dispatch = workflow.split('\n  dispatch:\n')[1].split('\n  sync:\n')[0]
  assert.doesNotMatch(dispatch, /environment:|secrets\./)
  assert.match(dispatch, /--ref "\$DEFAULT_BRANCH"/)
  assert.match(workflow, /\n  sync:\n    if: github\.event_name == 'workflow_dispatch'\n/)
})
