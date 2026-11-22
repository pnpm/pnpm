import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { afterEach, beforeEach, describe, test } from 'node:test'

import { findRepoRoot, parkPublishedPrivateChangelogs, parseSelectedProducts, releaseFilterArgs } from './bump.mjs'

describe('findRepoRoot', () => {
  let dir
  beforeEach(() => {
    dir = fs.mkdtempSync(path.join(os.tmpdir(), 'bump-root-'))
  })
  afterEach(() => fs.rmSync(dir, { recursive: true, force: true }))

  test('walks up to the directory containing .changeset', () => {
    fs.mkdirSync(path.join(dir, '.changeset'))
    const nested = path.join(dir, '.github', 'scripts')
    fs.mkdirSync(nested, { recursive: true })
    assert.equal(findRepoRoot(nested), dir)
  })

  test('throws when no .changeset directory exists above', () => {
    assert.throws(() => findRepoRoot(dir), /No \.changeset directory/)
  })
})

describe('parseSelectedProducts', () => {
  test('collects the products named by --release', () => {
    assert.deepEqual(parseSelectedProducts(['--release', 'pnpm', '--release', 'pnpr']), new Set(['pnpm', 'pnpr']))
  })

  test('is empty when no argument is passed', () => {
    assert.deepEqual(parseSelectedProducts([]), new Set())
  })

  test('skips the leading `--` separator forwarded by `pnpm run bump -- …`', () => {
    assert.deepEqual(parseSelectedProducts(['--', '--release', 'pnpm', '--release', 'pnpr']), new Set(['pnpm', 'pnpr']))
    assert.deepEqual(parseSelectedProducts(['--']), new Set())
  })

  test('rejects a `--` that is not in the leading position', () => {
    assert.throws(() => parseSelectedProducts(['--release', 'pnpm', '--']), /Unexpected bump argument/)
  })

  test('throws on an unknown product', () => {
    assert.throws(() => parseSelectedProducts(['--release', 'pnpm11']), /Unknown --release product/)
    assert.throws(() => parseSelectedProducts(['--release', 'toString']), /Unknown --release product/)
    assert.throws(() => parseSelectedProducts(['--release']), /Unknown --release product/)
  })

  test('fails closed on a misspelled flag instead of releasing everything', () => {
    assert.throws(() => parseSelectedProducts(['--releases', 'pnpr']), /Unexpected bump argument/)
  })
})

describe('releaseFilterArgs', () => {
  test('releases everything (no filter) when nothing is selected', () => {
    assert.deepEqual(releaseFilterArgs(new Set()), [])
  })

  test('includes only the selected products', () => {
    assert.deepEqual(releaseFilterArgs(new Set(['pnpm'])), ['--filter=pacquet', '--filter=@pnpm/napi'])
    assert.deepEqual(releaseFilterArgs(new Set(['pnpm', 'pnpr'])), ['--filter=pacquet', '--filter=@pnpm/napi', '--filter=@pnpm/pnpr'])
    assert.deepEqual(releaseFilterArgs(new Set(['get-pnpm'])), ['--filter=get-pnpm'])
  })
})

describe('parkPublishedPrivateChangelogs', () => {
  let dir
  beforeEach(() => {
    dir = fs.mkdtempSync(path.join(os.tmpdir(), 'bump-park-'))
  })
  afterEach(() => fs.rmSync(dir, { recursive: true, force: true }))

  function writeProject (projectDir, manifest, changelog) {
    fs.mkdirSync(path.join(dir, projectDir), { recursive: true })
    fs.writeFileSync(path.join(dir, projectDir, 'package.json'), JSON.stringify(manifest))
    if (changelog != null) {
      fs.writeFileSync(path.join(dir, projectDir, 'CHANGELOG.md'), changelog)
    }
  }

  const section = '## 12.8.0\n\n### Minor Changes\n\n- A change.\n\n### Patch Changes\n\n- A fix.\n'

  test('moves the committed changelog to the parked section of the released version', () => {
    writeProject('pnpm/npm/pnpm', { name: 'pacquet', version: '12.8.0' }, `# pacquet\n\n${section}`)
    writeProject('pnpm/npm/napi', { name: '@pnpm/napi', version: '12.8.0' }, '# @pnpm/napi\n\n## 12.8.0\n')

    parkPublishedPrivateChangelogs(dir, ['pnpm/npm/pnpm', 'pnpm/npm/napi'])

    assert.equal(fs.readFileSync(path.join(dir, '.changeset/changelogs/pacquet@12.8.0.md'), 'utf8'), section)
    assert.equal(fs.readFileSync(path.join(dir, '.changeset/changelogs/@pnpm!napi@12.8.0.md'), 'utf8'), '## 12.8.0\n')
    assert.equal(fs.existsSync(path.join(dir, 'pnpm/npm/pnpm/CHANGELOG.md')), false)
    assert.equal(fs.existsSync(path.join(dir, 'pnpm/npm/napi/CHANGELOG.md')), false)
  })

  test('skips a project the release did not bump', () => {
    writeProject('pnpr/npm/pnpr', { name: '@pnpm/pnpr', version: '0.1.0-alpha.14' })

    parkPublishedPrivateChangelogs(dir, ['pnpr/npm/pnpr'])

    assert.equal(fs.existsSync(path.join(dir, '.changeset/changelogs')), false)
  })

  test('refuses a changelog that holds more than the released section', () => {
    writeProject('pnpm/npm/pnpm', { name: 'pacquet', version: '12.9.0' }, `# pacquet\n\n## 12.9.0\n\n- New.\n\n${section}`)

    assert.throws(() => parkPublishedPrivateChangelogs(dir, ['pnpm/npm/pnpm']), /exactly one section, for pacquet@12\.9\.0/)
    assert.equal(fs.existsSync(path.join(dir, 'pnpm/npm/pnpm/CHANGELOG.md')), true)
  })

  test('refuses a changelog without a section for the released version', () => {
    writeProject('pnpm/npm/pnpm', { name: 'pacquet', version: '12.9.0' }, `# pacquet\n\n${section}`)

    assert.throws(() => parkPublishedPrivateChangelogs(dir, ['pnpm/npm/pnpm']), /exactly one section/)
  })
})
