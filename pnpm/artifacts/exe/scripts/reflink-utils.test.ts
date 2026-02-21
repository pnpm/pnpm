import assert from 'node:assert/strict'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { test, describe, beforeEach, afterEach } from 'node:test'
import { getReflinkKeepPackages, stripReflinkPackages } from './reflink-utils.ts'

describe('getReflinkKeepPackages()', () => {
  test('macos-arm64', () => {
    assert.deepEqual(
      getReflinkKeepPackages('macos-arm64'),
      ['@reflink/reflink-darwin-arm64']
    )
  })

  test('macos-x64', () => {
    assert.deepEqual(
      getReflinkKeepPackages('macos-x64'),
      ['@reflink/reflink-darwin-x64']
    )
  })

  test('win-x64', () => {
    assert.deepEqual(
      getReflinkKeepPackages('win-x64'),
      ['@reflink/reflink-win32-x64-msvc']
    )
  })

  test('win-arm64', () => {
    assert.deepEqual(
      getReflinkKeepPackages('win-arm64'),
      ['@reflink/reflink-win32-arm64-msvc']
    )
  })

  test('linux-x64 keeps both gnu and musl', () => {
    assert.deepEqual(
      getReflinkKeepPackages('linux-x64'),
      ['@reflink/reflink-linux-x64-gnu', '@reflink/reflink-linux-x64-musl']
    )
  })

  test('linux-arm64 keeps both gnu and musl', () => {
    assert.deepEqual(
      getReflinkKeepPackages('linux-arm64'),
      ['@reflink/reflink-linux-arm64-gnu', '@reflink/reflink-linux-arm64-musl']
    )
  })

  test('linuxstatic-x64 keeps both gnu and musl', () => {
    assert.deepEqual(
      getReflinkKeepPackages('linuxstatic-x64'),
      ['@reflink/reflink-linux-x64-gnu', '@reflink/reflink-linux-x64-musl']
    )
  })

  test('linuxstatic-arm64 keeps both gnu and musl', () => {
    assert.deepEqual(
      getReflinkKeepPackages('linuxstatic-arm64'),
      ['@reflink/reflink-linux-arm64-gnu', '@reflink/reflink-linux-arm64-musl']
    )
  })
})

describe('stripReflinkPackages()', () => {
  let tmpDir: string

  beforeEach(() => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'reflink-test-'))
  })

  afterEach(() => {
    fs.rmSync(tmpDir, { recursive: true })
  })

  function makeReflinkDir (...packages: string[]): string {
    const reflinkDir = path.join(tmpDir, 'node_modules', '@reflink')
    fs.mkdirSync(reflinkDir, { recursive: true })
    // Always create the main package
    fs.mkdirSync(path.join(reflinkDir, 'reflink'), { recursive: true })
    for (const pkg of packages) {
      fs.mkdirSync(path.join(reflinkDir, pkg), { recursive: true })
    }
    return reflinkDir
  }

  function listReflinkPackages (reflinkDir: string): string[] {
    return fs.readdirSync(reflinkDir).sort()
  }

  test('removes all platform packages when keepPackages is undefined', () => {
    const reflinkDir = makeReflinkDir('reflink-darwin-arm64', 'reflink-darwin-x64', 'reflink-linux-x64-gnu')
    stripReflinkPackages(tmpDir)
    assert.deepEqual(listReflinkPackages(reflinkDir), ['reflink'])
  })

  test('removes all platform packages when keepPackages is empty', () => {
    const reflinkDir = makeReflinkDir('reflink-darwin-arm64', 'reflink-win32-x64-msvc')
    stripReflinkPackages(tmpDir, [])
    assert.deepEqual(listReflinkPackages(reflinkDir), ['reflink'])
  })

  test('keeps only the specified packages', () => {
    const reflinkDir = makeReflinkDir(
      'reflink-darwin-arm64',
      'reflink-darwin-x64',
      'reflink-win32-x64-msvc',
      'reflink-linux-x64-gnu'
    )
    stripReflinkPackages(tmpDir, ['@reflink/reflink-darwin-arm64'])
    assert.deepEqual(listReflinkPackages(reflinkDir), ['reflink', 'reflink-darwin-arm64'])
  })

  test('always keeps the main @reflink/reflink package', () => {
    const reflinkDir = makeReflinkDir('reflink-darwin-arm64')
    stripReflinkPackages(tmpDir, [])
    assert.ok(fs.existsSync(path.join(reflinkDir, 'reflink')))
  })

  test('does nothing when node_modules/@reflink does not exist', () => {
    assert.doesNotThrow(() => stripReflinkPackages(tmpDir))
  })

  test('keeps linux gnu and musl for a linux target', () => {
    const reflinkDir = makeReflinkDir(
      'reflink-linux-x64-gnu',
      'reflink-linux-x64-musl',
      'reflink-darwin-arm64',
      'reflink-win32-x64-msvc'
    )
    stripReflinkPackages(tmpDir, getReflinkKeepPackages('linux-x64'))
    assert.deepEqual(listReflinkPackages(reflinkDir), [
      'reflink',
      'reflink-linux-x64-gnu',
      'reflink-linux-x64-musl',
    ])
  })
})
