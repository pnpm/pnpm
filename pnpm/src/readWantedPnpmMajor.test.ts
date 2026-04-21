import fs from 'fs'
import os from 'os'
import path from 'path'

import { readWantedPnpmMajor } from './readWantedPnpmMajor.js'

describe('readWantedPnpmMajor', () => {
  let tmpDir: string

  beforeEach(() => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-wanted-major-'))
  })

  afterEach(() => {
    fs.rmSync(tmpDir, { recursive: true, force: true })
  })

  function writeManifest (dir: string, manifest: unknown): void {
    fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify(manifest))
  }

  test('returns null when no package.json exists on the path', () => {
    // Nested empty dir, so walking up from it will hit this test's tmpDir
    // and eventually `/` without finding a package.json with packageManager.
    const nested = path.join(tmpDir, 'a', 'b')
    fs.mkdirSync(nested, { recursive: true })

    // We can't fully isolate from the real FS hierarchy (walkup eventually
    // hits `/`), so we assert the weaker property: no intermediate dir had
    // packageManager=pnpm@<major>.
    expect(readWantedPnpmMajor(nested)).toBeNull()
  })

  test('returns null when nearest package.json has no packageManager field', () => {
    writeManifest(tmpDir, { name: 'x', version: '1.0.0' })

    expect(readWantedPnpmMajor(tmpDir)).toBeNull()
  })

  test('returns null when packageManager is not pnpm', () => {
    writeManifest(tmpDir, { packageManager: 'yarn@4.0.0' })

    expect(readWantedPnpmMajor(tmpDir)).toBeNull()
  })

  test('returns the major version when packageManager is pnpm', () => {
    writeManifest(tmpDir, { packageManager: 'pnpm@10.33.0' })

    expect(readWantedPnpmMajor(tmpDir)).toBe(10)
  })

  test('returns the major version for a prerelease', () => {
    writeManifest(tmpDir, { packageManager: 'pnpm@11.0.0-rc.3' })

    expect(readWantedPnpmMajor(tmpDir)).toBe(11)
  })

  test('strips the integrity hash suffix', () => {
    writeManifest(tmpDir, { packageManager: 'pnpm@11.0.0+sha256.abc123' })

    expect(readWantedPnpmMajor(tmpDir)).toBe(11)
  })

  test('walks up to an ancestor package.json', () => {
    writeManifest(tmpDir, { packageManager: 'pnpm@11.0.0' })
    const nested = path.join(tmpDir, 'packages', 'foo')
    fs.mkdirSync(nested, { recursive: true })

    expect(readWantedPnpmMajor(nested)).toBe(11)
  })

  test('walks up past a nested package.json without packageManager to an ancestor', () => {
    writeManifest(tmpDir, { packageManager: 'pnpm@11.0.0' })
    const nested = path.join(tmpDir, 'packages', 'foo')
    fs.mkdirSync(nested, { recursive: true })
    writeManifest(nested, { name: 'foo', version: '1.0.0' })

    expect(readWantedPnpmMajor(nested)).toBe(11)
  })

  test('respects a nested package.json that declares a non-pnpm packageManager', () => {
    writeManifest(tmpDir, { packageManager: 'pnpm@11.0.0' })
    const nested = path.join(tmpDir, 'packages', 'foo')
    fs.mkdirSync(nested, { recursive: true })
    writeManifest(nested, { packageManager: 'yarn@4.0.0' })

    expect(readWantedPnpmMajor(nested)).toBeNull()
  })

  test('returns null for malformed JSON', () => {
    fs.writeFileSync(path.join(tmpDir, 'package.json'), '{not json')

    expect(readWantedPnpmMajor(tmpDir)).toBeNull()
  })
})
