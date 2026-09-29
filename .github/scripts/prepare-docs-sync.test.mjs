import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { test } from 'node:test'
import { prepareDocsSync } from './prepare-docs-sync.mjs'

function fixture (t, tag = 'v12.8.2') {
  const repo = mkdtempSync(path.join(os.tmpdir(), 'pnpm-docs-release-'))
  t.after(() => rmSync(repo, { recursive: true, force: true }))
  const git = (...args) => execFileSync('git', args, { cwd: repo, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim()
  const version = tag.slice(1)
  for (const [product, current] of [['pnpm/npm/pnpm', '12.8.2'], ['pnpm11/pnpm', '11.28.2']]) {
    mkdirSync(path.join(repo, product), { recursive: true })
    const productVersion = version.split('.')[0] === current.split('.')[0] ? version : current
    writeFileSync(path.join(repo, product, 'package.json'), JSON.stringify({ version: productVersion }))
  }
  git('init', '--quiet')
  git('config', 'user.name', 'Test')
  git('config', 'user.email', 'test@example.com')
  git('add', '.')
  git('commit', '--quiet', '-m', 'chore: release')
  git('update-ref', `refs/tags/${tag}`, 'HEAD')
  const sha = git('rev-parse', 'HEAD')
  let verified = false
  const options = {
    eventName: 'workflow_run',
    event: { workflow_run: { head_branch: tag, head_sha: sha } },
    git: (...args) => {
      if (args[0] === 'verify-tag') { verified = true; return '' }
      return git(...args)
    },
    publicationState: spec => {
      assert.equal(verified, true)
      assert.equal(spec, `pnpm@${version}`)
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
  const event = { workflow_run: { ...f.options.event.workflow_run, head_sha: 'a'.repeat(40) } }
  assert.throws(() => prepareDocsSync({ ...f.options, event }), /does not match/)
})

test('ignores verification runs and releases outside stable v11 and v12', () => {
  assert.equal(prepareDocsSync({ eventName: 'workflow_run', event: { workflow_run: { head_branch: 'main' } } }), undefined)
  assert.equal(prepareDocsSync({ eventName: 'workflow_dispatch', releaseTag: 'v12.9.0-beta.1' }), undefined)
  for (const tag of ['pnpr@0.1.0-alpha.15', 'v10.30.0', 'v13.0.0']) {
    assert.equal(prepareDocsSync({ eventName: 'workflow_run', event: { workflow_run: { head_branch: tag } } }), undefined)
    assert.throws(() => prepareDocsSync({ eventName: 'workflow_dispatch', releaseTag: tag }), /v11 or v12/)
  }
})

test('manual corrections must contain only the selected version docs', t => {
  const f = fixture(t)
  mkdirSync(path.join(f.repo, 'pnpm/docs'), { recursive: true })
  writeFileSync(path.join(f.repo, 'pnpm/docs/install.md'), 'correction')
  f.git('add', '.')
  f.git('commit', '--quiet', '-m', 'docs: correction')
  const options = { ...f.options, eventName: 'workflow_dispatch', releaseTag: 'v12.8.2', docsRef: f.git('rev-parse', 'HEAD') }
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
  const result = prepareDocsSync({ ...f.options, eventName: 'workflow_dispatch', releaseTag: 'v11.28.3', docsRef: f.git('rev-parse', 'HEAD') })
  assert.equal(result.line, '11.x')
})
