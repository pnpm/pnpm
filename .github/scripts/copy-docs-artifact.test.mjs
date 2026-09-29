import assert from 'node:assert/strict'
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

test('rejects executable Git metadata and files outside documentation before changing the checkout', t => {
  const f = fixture(t)
  for (const file of ['.git/hooks/pre-commit', 'docs/.gitattributes', 'docs/.git/config', 'package.json', '.npmrc']) {
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
