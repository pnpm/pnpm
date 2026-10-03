import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, beforeEach, describe, expect, test } from '@jest/globals'
import { detectIfCurrentPkgIsExecutable, getCurrentPackageName, homebrewFormulaOf, isExecutedByCorepack } from '@pnpm/cli.meta'

describe('detectIfCurrentPkgIsExecutable()', () => {
  test('returns false when not running as a SEA binary', () => {
    // In a test environment node:sea is unavailable, so the require() inside
    // detectIfCurrentPkgIsExecutable() throws and the catch block returns false.
    expect(detectIfCurrentPkgIsExecutable()).toBe(false)
  })
})

describe('getCurrentPackageName()', () => {
  test('returns "pnpm" when not running as a SEA binary', () => {
    expect(getCurrentPackageName()).toBe('pnpm')
  })
})

describe('isExecutedByCorepack()', () => {
  test('returns true when COREPACK_ROOT is set', () => {
    expect(isExecutedByCorepack({ COREPACK_ROOT: '/usr/local/lib/corepack' })).toBe(true)
  })

  test('returns false when COREPACK_ROOT is not set', () => {
    expect(isExecutedByCorepack({})).toBe(false)
  })

  test('returns false when COREPACK_ROOT is undefined', () => {
    expect(isExecutedByCorepack({ COREPACK_ROOT: undefined })).toBe(false)
  })
})

describe('homebrewFormulaOf()', () => {
  let root: string
  beforeEach(() => {
    root = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-cli-meta-'))
  })
  afterEach(() => {
    fs.rmSync(root, { recursive: true, force: true })
  })

  function createKeg (formula: string, { receipt }: { receipt: boolean }): string {
    const keg = path.join(root, 'Cellar', formula, '1.0.0')
    const entry = path.join(keg, 'lib', 'node_modules', 'pnpm', 'dist', 'pnpm.mjs')
    fs.mkdirSync(path.dirname(entry), { recursive: true })
    if (receipt) fs.writeFileSync(path.join(keg, 'INSTALL_RECEIPT.json'), '{}')
    return entry
  }

  test('returns the formula whose keg holds the path', () => {
    expect(homebrewFormulaOf(createKeg('pnpm', { receipt: true }))).toBe('pnpm')
    expect(homebrewFormulaOf(createKeg('pnpm@11', { receipt: true }))).toBe('pnpm@11')
  })

  test('returns undefined outside a pnpm keg', () => {
    expect(homebrewFormulaOf(createKeg('node', { receipt: true }))).toBeUndefined()
    expect(homebrewFormulaOf(createKeg('pnpm@10', { receipt: false }))).toBeUndefined()
    expect(homebrewFormulaOf(path.join(root, 'pnpm.mjs'))).toBeUndefined()
  })
})
