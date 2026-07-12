import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, beforeEach, describe, expect, test } from '@jest/globals'

import {
  findRepoRoot,
  syncRustVersions,
} from '../src/bump.js'

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

function writeManifest (repoRoot: string, manifestPath: string, manifest: object): void {
  const abs = path.join(repoRoot, manifestPath)
  fs.mkdirSync(path.dirname(abs), { recursive: true })
  fs.writeFileSync(abs, `${JSON.stringify(manifest, null, 2)}\n`)
}

describe('syncRustVersions', () => {
  let dir: string
  beforeEach(() => {
    dir = fs.mkdtempSync(path.join(os.tmpdir(), 'bump-sync-'))
    writeManifest(dir, 'pnpm/npm/pnpm/package.json', { name: 'pnpm', version: '12.0.0-alpha.9' })
    writeManifest(dir, 'pnpr/npm/pnpr/package.json', { name: '@pnpm/pnpr', version: '0.2.0' })
    fs.mkdirSync(path.join(dir, 'pnpm/crates/config/src'), { recursive: true })
    fs.writeFileSync(
      path.join(dir, 'pnpm/crates/config/src/defaults.rs'),
      'pub const PNPM_VERSION: &str = "12.0.0-alpha.8";\n'
    )
    fs.mkdirSync(path.join(dir, 'pnpr/crates/pnpr'), { recursive: true })
    fs.writeFileSync(
      path.join(dir, 'pnpr/crates/pnpr/Cargo.toml'),
      '[package]\nname              = "pnpr"\nversion           = "0.1.0"\n'
    )
    fs.writeFileSync(
      path.join(dir, 'Cargo.lock'),
      '[[package]]\nname = "pnpr"\nversion = "0.1.0"\ndependencies = [\n "clap",\n]\n\n[[package]]\nname = "other"\nversion = "0.1.0"\n'
    )
  })
  afterEach(() => fs.rmSync(dir, { recursive: true, force: true }))

  test('copies the wrapper versions into the Rust sources', () => {
    syncRustVersions(dir)

    expect(fs.readFileSync(path.join(dir, 'pnpm/crates/config/src/defaults.rs'), 'utf8'))
      .toContain('pub const PNPM_VERSION: &str = "12.0.0-alpha.9";')
    expect(fs.readFileSync(path.join(dir, 'pnpr/crates/pnpr/Cargo.toml'), 'utf8'))
      .toContain('version           = "0.2.0"')
    const lock = fs.readFileSync(path.join(dir, 'Cargo.lock'), 'utf8')
    expect(lock).toContain('name = "pnpr"\nversion = "0.2.0"')
    // Only the pnpr package entry is touched, not other packages at the same version.
    expect(lock).toContain('name = "other"\nversion = "0.1.0"')
  })

  test('throws when an expected version site is missing', () => {
    fs.writeFileSync(path.join(dir, 'pnpm/crates/config/src/defaults.rs'), '// gone\n')
    expect(() => syncRustVersions(dir)).toThrow(/not found in .*defaults\.rs/)
  })
})
