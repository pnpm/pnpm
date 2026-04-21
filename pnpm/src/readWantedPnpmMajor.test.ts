import fs from 'fs'
import os from 'os'
import path from 'path'

import {
  readManagePackageManagerVersionsSetting,
  readWantedPnpmMajor,
  shouldSkipNpmPassthrough,
} from './readWantedPnpmMajor.js'

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

  test('returns null when packageManager is present but not a string', () => {
    writeManifest(tmpDir, { packageManager: 123 })

    expect(readWantedPnpmMajor(tmpDir)).toBeNull()
  })

  test('returns null when the manifest root is not a plain object', () => {
    fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(['pnpm@11.0.0']))

    expect(readWantedPnpmMajor(tmpDir)).toBeNull()
  })
})

describe('readManagePackageManagerVersionsSetting', () => {
  let tmpDir: string

  beforeEach(() => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-manage-setting-'))
  })

  afterEach(() => {
    fs.rmSync(tmpDir, { recursive: true, force: true })
  })

  test('returns null when no .npmrc on the path sets the flag', () => {
    const nested = path.join(tmpDir, 'a', 'b')
    fs.mkdirSync(nested, { recursive: true })

    // Same caveat as the packageManager tests: walkup eventually hits `/`,
    // so we assert the weaker property that none of the intermediate .npmrc
    // files set this flag.
    expect(readManagePackageManagerVersionsSetting(nested)).toBeNull()
  })

  test('reads an explicit false from .npmrc in cwd', () => {
    fs.writeFileSync(path.join(tmpDir, '.npmrc'), 'manage-package-manager-versions=false\n')

    expect(readManagePackageManagerVersionsSetting(tmpDir)).toBe(false)
  })

  test('reads an explicit true from .npmrc in cwd', () => {
    fs.writeFileSync(path.join(tmpDir, '.npmrc'), 'manage-package-manager-versions=true\n')

    expect(readManagePackageManagerVersionsSetting(tmpDir)).toBe(true)
  })

  test('walks up to an ancestor .npmrc', () => {
    fs.writeFileSync(path.join(tmpDir, '.npmrc'), 'manage-package-manager-versions=false\n')
    const nested = path.join(tmpDir, 'packages', 'foo')
    fs.mkdirSync(nested, { recursive: true })

    expect(readManagePackageManagerVersionsSetting(nested)).toBe(false)
  })

  test('nearer .npmrc wins over an ancestor', () => {
    fs.writeFileSync(path.join(tmpDir, '.npmrc'), 'manage-package-manager-versions=false\n')
    const nested = path.join(tmpDir, 'packages', 'foo')
    fs.mkdirSync(nested, { recursive: true })
    fs.writeFileSync(path.join(nested, '.npmrc'), 'manage-package-manager-versions=true\n')

    expect(readManagePackageManagerVersionsSetting(nested)).toBe(true)
  })

  test('ignores .npmrc files that do not set the flag', () => {
    fs.writeFileSync(path.join(tmpDir, '.npmrc'), 'registry=https://example.com/\n')
    const nested = path.join(tmpDir, 'packages', 'foo')
    fs.mkdirSync(nested, { recursive: true })
    fs.writeFileSync(path.join(nested, '.npmrc'), 'something-else=1\n')

    expect(readManagePackageManagerVersionsSetting(nested)).toBeNull()
  })
})

describe('shouldSkipNpmPassthrough', () => {
  let tmpDir: string

  beforeEach(() => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-skip-decision-'))
  })

  afterEach(() => {
    fs.rmSync(tmpDir, { recursive: true, force: true })
  })

  function writeManifest (dir: string, manifest: unknown): void {
    fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify(manifest))
  }

  test('skips passthrough when packageManager wants pnpm v11+ and nothing disables switching', () => {
    writeManifest(tmpDir, { packageManager: 'pnpm@11.0.0' })

    expect(shouldSkipNpmPassthrough({}, tmpDir)).toBe(true)
  })

  test('does not skip when packageManager wants pnpm v10', () => {
    writeManifest(tmpDir, { packageManager: 'pnpm@10.33.0' })

    expect(shouldSkipNpmPassthrough({}, tmpDir)).toBe(false)
  })

  test('does not skip when packageManager selects a non-pnpm manager', () => {
    writeManifest(tmpDir, { packageManager: 'yarn@4.0.0' })

    expect(shouldSkipNpmPassthrough({}, tmpDir)).toBe(false)
  })

  test('does not skip when COREPACK_ROOT is set', () => {
    writeManifest(tmpDir, { packageManager: 'pnpm@11.0.0' })

    expect(shouldSkipNpmPassthrough({ COREPACK_ROOT: '/some/path' }, tmpDir)).toBe(false)
  })

  test('does not skip when npm_config_manage_package_manager_versions=false in env', () => {
    writeManifest(tmpDir, { packageManager: 'pnpm@11.0.0' })

    expect(shouldSkipNpmPassthrough({ npm_config_manage_package_manager_versions: 'false' }, tmpDir)).toBe(false)
  })

  test('does not skip when manage-package-manager-versions=false in .npmrc', () => {
    writeManifest(tmpDir, { packageManager: 'pnpm@11.0.0' })
    fs.writeFileSync(path.join(tmpDir, '.npmrc'), 'manage-package-manager-versions=false\n')

    expect(shouldSkipNpmPassthrough({}, tmpDir)).toBe(false)
  })

  test('skips when .npmrc explicitly sets manage-package-manager-versions=true', () => {
    writeManifest(tmpDir, { packageManager: 'pnpm@11.0.0' })
    fs.writeFileSync(path.join(tmpDir, '.npmrc'), 'manage-package-manager-versions=true\n')

    expect(shouldSkipNpmPassthrough({}, tmpDir)).toBe(true)
  })
})
