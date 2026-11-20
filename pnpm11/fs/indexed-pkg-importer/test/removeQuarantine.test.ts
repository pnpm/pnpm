import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, beforeEach, describe, expect, it, jest, test } from '@jest/globals'

const globalWarn = jest.fn()
jest.unstable_mockModule('@pnpm/logger', () => ({ globalWarn }))

const { isNativeBinary, removeQuarantine } = await import('../src/removeQuarantine.js')

// Quarantine xattrs only exist on macOS, so these tests are scoped to it.
const describeOnMacOS = process.platform === 'darwin' ? describe : describe.skip

const QUARANTINE_ATTR = 'com.apple.quarantine'

function setQuarantine (filePath: string): void {
  execFileSync('/usr/bin/xattr', ['-w', QUARANTINE_ATTR, '0083;00000000;TestApp;', filePath])
}

function listXattrs (filePath: string): string {
  return execFileSync('/usr/bin/xattr', ['-l', filePath], { encoding: 'utf8' })
}

function hasQuarantine (filePath: string): boolean {
  return listXattrs(filePath).includes(QUARANTINE_ATTR)
}

test('isNativeBinary matches only native binary extensions handled on macOS', () => {
  expect(isNativeBinary('rollup.darwin-arm64.node')).toBe(true)
  expect(isNativeBinary('addon.DYLIB')).toBe(true)
  expect(isNativeBinary('addon.so')).toBe(true)
  expect(isNativeBinary('index.js')).toBe(false)
  expect(isNativeBinary('package.json')).toBe(false)
  expect(isNativeBinary('README')).toBe(false)
  // .dll is Windows-only and never relevant on macOS.
  expect(isNativeBinary('addon.dll')).toBe(false)
})

describeOnMacOS('removeQuarantine', () => {
  let testDir: string

  beforeEach(() => {
    globalWarn.mockClear()
    testDir = fs.mkdtempSync(path.join(os.tmpdir(), 'quarantine-test-'))
  })

  afterEach(() => {
    fs.rmSync(testDir, { recursive: true, force: true })
  })

  it('removes the quarantine xattr from a file', () => {
    const file = path.join(testDir, 'addon.node')
    fs.writeFileSync(file, 'test content')
    setQuarantine(file)
    expect(hasQuarantine(file)).toBe(true)

    removeQuarantine([file])

    expect(hasQuarantine(file)).toBe(false)
    expect(globalWarn).not.toHaveBeenCalled()
  })

  it('does nothing when the quarantine xattr is absent', () => {
    const file = path.join(testDir, 'addon.node')
    fs.writeFileSync(file, 'test content')
    expect(hasQuarantine(file)).toBe(false)

    expect(() => removeQuarantine([file])).not.toThrow()
    expect(globalWarn).not.toHaveBeenCalled()
  })

  it('removes quarantine from a batch of files while preserving other xattrs', () => {
    const quarantined = path.join(testDir, 'a.node')
    const clean = path.join(testDir, 'b.node')
    fs.writeFileSync(quarantined, 'a')
    fs.writeFileSync(clean, 'b')
    setQuarantine(quarantined)
    execFileSync('/usr/bin/xattr', ['-w', 'com.example.custom', 'keep', quarantined])

    removeQuarantine([quarantined, clean])

    expect(hasQuarantine(quarantined)).toBe(false)
    expect(listXattrs(quarantined)).toContain('com.example.custom')
    expect(globalWarn).not.toHaveBeenCalled()
  })

  it('does not warn for missing files (dropped/renamed by the importer)', () => {
    const quarantined = path.join(testDir, 'real.node')
    const missing = path.join(testDir, 'dropped.node')
    fs.writeFileSync(quarantined, 'a')
    setQuarantine(quarantined)

    removeQuarantine([missing, quarantined])

    expect(hasQuarantine(quarantined)).toBe(false)
    expect(globalWarn).not.toHaveBeenCalled()
  })

  it('does not throw when given an empty list', () => {
    expect(() => removeQuarantine([])).not.toThrow()
  })
})
