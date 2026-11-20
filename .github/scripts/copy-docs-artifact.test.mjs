import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { test } from 'node:test'
import { copyDocsArtifact } from './copy-docs-artifact.mjs'

function fixture (t) {
  const root = mkdtempSync(path.join(os.tmpdir(), 'docs-artifact-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  const source = path.join(root, 'source')
  const destination = path.join(root, 'website')
  const write = (base, file, text) => {
    mkdirSync(path.dirname(path.join(base, file)), { recursive: true })
    writeFileSync(path.join(base, file), text)
  }
  for (const file of ['docs/index.md', 'pnpr-docs/index.md', 'versioned_docs/version-11.x/index.md', 'sidebars.json', 'sidebars-pnpr.json', 'versioned_sidebars/version-11.x-sidebars.json', 'docs-sync.json']) write(source, file, 'generated')
  write(destination, 'docs/removed.md', 'old')
  write(destination, 'blog/post.md', 'blog')
  write(destination, 'blog/releases/12.8.md', 'hand-written')
  write(destination, '.git/config', 'trusted config')
  return { source, destination, write }
}

test('copies generated docs, removes stale files and preserves website and Git files', t => {
  const f = fixture(t)
  f.write(f.source, 'static/docs-assets/12.x/img/logo.png', 'image')
  copyDocsArtifact(f.source, f.destination)
  assert.equal(readFileSync(path.join(f.destination, 'docs/index.md'), 'utf8'), 'generated')
  assert.equal(readFileSync(path.join(f.destination, 'static/docs-assets/12.x/img/logo.png'), 'utf8'), 'image')
  assert.equal(existsSync(path.join(f.destination, 'docs/removed.md')), false)
  assert.equal(readFileSync(path.join(f.destination, 'blog/post.md'), 'utf8'), 'blog')
  assert.equal(readFileSync(path.join(f.destination, '.git/config'), 'utf8'), 'trusted config')
})

test('adds release pages without removing the existing ones', t => {
  const f = fixture(t)
  f.write(f.source, 'blog/releases/12.9.0.md', 'generated')
  copyDocsArtifact(f.source, f.destination)
  assert.equal(readFileSync(path.join(f.destination, 'blog/releases/12.9.0.md'), 'utf8'), 'generated')
  assert.equal(readFileSync(path.join(f.destination, 'blog/releases/12.8.md'), 'utf8'), 'hand-written')
})

test('rejects executable Git metadata and files outside documentation before changing the checkout', t => {
  const f = fixture(t)
  for (const file of ['.git/hooks/pre-commit', 'docs/.gitattributes', 'docs/.git/config', 'package.json', '.npmrc', 'blog/post.md', 'blog/releases/nested/page.md', 'blog/releases/page.js']) {
    f.write(f.source, file, 'malicious')
    assert.throws(() => copyDocsArtifact(f.source, f.destination), /Unexpected documentation artifact/)
    assert.equal(readFileSync(path.join(f.destination, 'docs/removed.md'), 'utf8'), 'old')
    rmSync(path.join(f.source, file))
  }
})

test('rejects symlinks and incomplete artifacts before deleting published docs', t => {
  const f = fixture(t)
  const link = path.join(f.source, 'docs/link.md')
  symlinkSync(path.join(f.destination, '.git/config'), link)
  assert.throws(() => copyDocsArtifact(f.source, f.destination), /Unexpected documentation artifact/)
  rmSync(link)
  rmSync(path.join(f.source, 'sidebars.json'))
  assert.throws(() => copyDocsArtifact(f.source, f.destination), /Missing documentation artifact/)
  assert.equal(readFileSync(path.join(f.destination, 'docs/removed.md'), 'utf8'), 'old')
})

test('the workflow enables pushing only after creating a documentation commit', t => {
  const f = fixture(t)
  f.write(f.source, 'static/docs-assets/12.x/img/logo.svg', '<svg/>')
  f.write(f.source, 'blog/releases/12.8.md', 'hand-written')
  const workflow = readFileSync(new URL('../workflows/sync-docs.yml', import.meta.url), 'utf8')
  const commitStep = workflow.split('      - name: Commit the generated documentation\n')[1].split('      - name: Push the verified documentation commit\n')[0]
  assert.match(commitStep, /        id: commit\n/)
  assert.match(workflow, /      - name: Push the verified documentation commit\n        if: steps\.commit\.outputs\.changed == 'true'/)
  const script = commitStep.split('        run: |\n')[1].trimEnd().split('\n').map(line => line.slice(10)).join('\n')
  const git = (...args) => execFileSync('git', args, { cwd: f.source, encoding: 'utf8', stdio: 'pipe' }).trim()
  git('init', '--quiet')
  git('config', 'user.name', 'Test')
  git('config', 'user.email', 'test@example.invalid')
  git('config', 'commit.gpgsign', 'false')
  git('add', '.')
  git('commit', '--quiet', '-m', 'test: initial documentation')
  const original = git('rev-parse', 'HEAD')
  const output = path.join(f.source, 'workflow-output')
  const run = () => execFileSync('bash', ['-e', '-c', script], {
    cwd: f.source,
    env: { ...process.env, GITHUB_OUTPUT: output, MAIN_SYNC: 'true', DOCS_COMMIT: original, LINE: '', VERSION: '', GH_TOKEN: '' },
    stdio: 'pipe',
  })
  run()
  assert.equal(git('rev-parse', 'HEAD'), original)
  assert.equal(existsSync(output), false)
  f.write(f.source, 'docs/index.md', 'updated')
  run()
  assert.notEqual(git('rev-parse', 'HEAD'), original)
  assert.equal(readFileSync(output, 'utf8'), 'changed=true\n')
})
