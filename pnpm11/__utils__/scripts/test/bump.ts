import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, beforeEach, describe, expect, test } from '@jest/globals'

import { findRepoRoot, parkPublishedPrivateChangelogs, parseSelectedProducts, releaseFilterArgs } from '../src/bump.js'

describe('findRepoRoot', () => {
  let dir: string
  beforeEach(() => {
    dir = fs.mkdtempSync(path.join(os.tmpdir(), 'bump-root-'))
  })
  afterEach(() => fs.rmSync(dir, { recursive: true, force: true }))

  test('walks up to the directory containing .changeset', () => {
    fs.mkdirSync(path.join(dir, '.changeset'))
    const nested = path.join(dir, 'pnpm11', '__utils__', 'scripts', 'src')
    fs.mkdirSync(nested, { recursive: true })
    expect(findRepoRoot(nested)).toBe(dir)
  })

  test('throws when no .changeset directory exists above', () => {
    expect(() => findRepoRoot(dir)).toThrow(/No \.changeset directory/)
  })
})

describe('parseSelectedProducts', () => {
  test('collects the products named by --release', () => {
    expect(parseSelectedProducts(['--release', 'pnpm11', '--release', 'pnpr']))
      .toEqual(new Set(['pnpm11', 'pnpr']))
  })

  test('is empty when no argument is passed (a bare `pnpm bump` releases everything)', () => {
    expect(parseSelectedProducts([])).toEqual(new Set())
  })

  test('skips the leading `--` separator forwarded by `pnpm run bump -- …`', () => {
    expect(parseSelectedProducts(['--', '--release', 'pnpm', '--release', 'pnpr']))
      .toEqual(new Set(['pnpm', 'pnpr']))
    expect(parseSelectedProducts(['--'])).toEqual(new Set())
  })

  test('rejects a `--` that is not in the leading position', () => {
    expect(() => parseSelectedProducts(['--release', 'pnpm', '--'])).toThrow(/Unexpected bump argument/)
  })

  test('throws on an unknown product', () => {
    expect(() => parseSelectedProducts(['--release', 'bogus'])).toThrow(/Unknown --release product/)
  })

  test('fails closed on a misspelled flag instead of releasing everything', () => {
    expect(() => parseSelectedProducts(['--releases', 'pnpr'])).toThrow(/Unexpected bump argument/)
  })
})

describe('releaseFilterArgs', () => {
  test('releases everything (no filter) when nothing is selected', () => {
    expect(releaseFilterArgs(new Set())).toEqual([])
  })

  test('releases everything (no filter) when all three products are selected', () => {
    expect(releaseFilterArgs(new Set(['pnpm11', 'pnpm', 'pnpr']))).toEqual([])
  })

  test('excludes the unselected alpha products when pnpm11 is selected', () => {
    expect(releaseFilterArgs(new Set(['pnpm11'])))
      .toEqual(['--filter=!pacquet', '--filter=!@pnpm/napi', '--filter=!@pnpm/pnpr'])
    expect(releaseFilterArgs(new Set(['pnpm11', 'pnpr'])))
      .toEqual(['--filter=!pacquet', '--filter=!@pnpm/napi'])
  })

  test('includes only the selected alpha products when pnpm11 is not selected', () => {
    expect(releaseFilterArgs(new Set(['pnpm'])))
      .toEqual(['--filter=pacquet', '--filter=@pnpm/napi'])
    expect(releaseFilterArgs(new Set(['pnpm', 'pnpr'])))
      .toEqual(['--filter=pacquet', '--filter=@pnpm/napi', '--filter=@pnpm/pnpr'])
    expect(releaseFilterArgs(new Set(['pnpr'])))
      .toEqual(['--filter=@pnpm/pnpr'])
  })
})

describe('parkPublishedPrivateChangelogs', () => {
  let dir: string
  beforeEach(() => {
    dir = fs.mkdtempSync(path.join(os.tmpdir(), 'bump-park-'))
  })
  afterEach(() => fs.rmSync(dir, { recursive: true, force: true }))

  function writeProject (projectDir: string, manifest: { name: string, version: string }, changelog?: string): void {
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

    expect(fs.readFileSync(path.join(dir, '.changeset/changelogs/pacquet@12.8.0.md'), 'utf8')).toBe(section)
    expect(fs.readFileSync(path.join(dir, '.changeset/changelogs/@pnpm!napi@12.8.0.md'), 'utf8')).toBe('## 12.8.0\n')
    expect(fs.existsSync(path.join(dir, 'pnpm/npm/pnpm/CHANGELOG.md'))).toBe(false)
    expect(fs.existsSync(path.join(dir, 'pnpm/npm/napi/CHANGELOG.md'))).toBe(false)
  })

  test('skips a project the release did not bump', () => {
    writeProject('pnpr/npm/pnpr', { name: '@pnpm/pnpr', version: '0.1.0-alpha.14' })

    parkPublishedPrivateChangelogs(dir, ['pnpr/npm/pnpr'])

    expect(fs.existsSync(path.join(dir, '.changeset/changelogs'))).toBe(false)
  })

  test('refuses a changelog that holds more than the released section', () => {
    writeProject('pnpm/npm/pnpm', { name: 'pacquet', version: '12.9.0' }, `# pacquet\n\n## 12.9.0\n\n- New.\n\n${section}`)

    expect(() => parkPublishedPrivateChangelogs(dir, ['pnpm/npm/pnpm'])).toThrow(/exactly one section, for pacquet@12\.9\.0/)
    expect(fs.existsSync(path.join(dir, 'pnpm/npm/pnpm/CHANGELOG.md'))).toBe(true)
  })

  test('refuses a changelog that holds an older section before the released one', () => {
    writeProject('pnpm/npm/pnpm', { name: 'pacquet', version: '12.8.0' }, `# pacquet\n\n## 12.7.0\n\n- Old.\n\n${section}`)

    expect(() => parkPublishedPrivateChangelogs(dir, ['pnpm/npm/pnpm'])).toThrow(/exactly one section, for pacquet@12\.8\.0/)
    expect(fs.existsSync(path.join(dir, 'pnpm/npm/pnpm/CHANGELOG.md'))).toBe(true)
  })

  test('refuses a changelog without a section for the released version', () => {
    writeProject('pnpm/npm/pnpm', { name: 'pacquet', version: '12.9.0' }, `# pacquet\n\n${section}`)

    expect(() => parkPublishedPrivateChangelogs(dir, ['pnpm/npm/pnpm'])).toThrow(/exactly one section/)
  })
})
