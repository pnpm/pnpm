// cspell:ignore WDAC
import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, expect, jest, test } from '@jest/globals'
import { renameFileWithRetry } from '@pnpm/fs.graceful-fs'

const platform = Object.getOwnPropertyDescriptor(process, 'platform')!

afterEach(() => {
  jest.restoreAllMocks()
  Object.defineProperty(process, 'platform', platform)
})

test.each(['EPERM', 'EACCES', 'EBUSY'])('%s uses its bounded Windows retry budget', (code) => {
  Object.defineProperty(process, 'platform', { value: 'win32' })
  let elapsed = 0
  jest.spyOn(Date, 'now').mockImplementation(() => elapsed)
  jest.spyOn(Atomics, 'wait').mockImplementation((_array, _index, _value, delay) => {
    elapsed += delay!
    return 'timed-out'
  })
  const error = Object.assign(new Error('rename failed'), { code })
  jest.spyOn(fs, 'renameSync').mockImplementation(() => {
    throw error
  })

  expect(() => renameFileWithRetry('source', 'destination')).toThrow(error)
  expect(elapsed).toBe(code === 'EBUSY' ? 60_000 : 1_000)
})

test('a permission error keeps the short deadline when subsequent errors are busy', () => {
  Object.defineProperty(process, 'platform', { value: 'win32' })
  let elapsed = 0
  jest.spyOn(Date, 'now').mockImplementation(() => elapsed)
  jest.spyOn(Atomics, 'wait').mockImplementation((_array, _index, _value, delay) => {
    elapsed += delay!
    return 'timed-out'
  })
  const denied = Object.assign(new Error('denied'), { code: 'EPERM' })
  const busy = Object.assign(new Error('busy'), { code: 'EBUSY' })
  jest.spyOn(fs, 'renameSync')
    .mockImplementationOnce(() => {
      throw denied
    })
    .mockImplementation(() => {
      throw busy
    })

  expect(() => renameFileWithRetry('source', 'destination')).toThrow(busy)
  expect(elapsed).toBe(1_000)
})

test('rename recovers from a temporary permission error', () => {
  Object.defineProperty(process, 'platform', { value: 'win32' })
  const rename = jest.spyOn(fs, 'renameSync')
    .mockImplementationOnce(() => {
      throw Object.assign(new Error('denied'), { code: 'EPERM' })
    })
    .mockImplementation(() => {})

  renameFileWithRetry('source', 'destination')
  expect(rename).toHaveBeenCalledTimes(2)
})

test('Unix permission errors are returned immediately', () => {
  Object.defineProperty(process, 'platform', { value: 'linux' })
  const error = Object.assign(new Error('denied'), { code: 'EACCES' })
  const rename = jest.spyOn(fs, 'renameSync').mockImplementation(() => {
    throw error
  })

  expect(() => renameFileWithRetry('source', 'destination')).toThrow(error)
  expect(rename).toHaveBeenCalledTimes(1)
})

const windowsTest = process.platform === 'win32' ? test : test.skip

windowsTest('a read-only destination fails promptly and preserves both files', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'rename-retry-'))
  const source = path.join(root, 'source')
  const destination = path.join(root, 'destination')
  try {
    fs.writeFileSync(source, 'replacement')
    fs.writeFileSync(destination, 'preserved')
    fs.chmodSync(destination, 0o444)
    const started = Date.now()
    expect(() => renameFileWithRetry(source, destination)).toThrow()
    expect(Date.now() - started).toBeLessThan(5_000)
    expect(fs.readFileSync(source, 'utf8')).toBe('replacement')
    expect(fs.readFileSync(destination, 'utf8')).toBe('preserved')
  } finally {
    fs.chmodSync(destination, 0o666)
    fs.rmSync(root, { recursive: true })
  }
})

windowsTest('a restrictive ACL fails promptly and preserves the source', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'rename-retry-'))
  const source = path.join(root, 'source')
  const destination = path.join(root, 'destination')
  fs.writeFileSync(source, 'preserved')
  try {
    execFileSync('icacls', [root, '/inheritance:r', '/grant:r', '*S-1-1-0:(RX,WDAC)'])
    execFileSync('icacls', [source, '/inheritance:r', '/grant:r', '*S-1-1-0:(R,WDAC)'])
    expect(() => fs.renameSync(source, destination)).toThrow()
    const started = Date.now()
    expect(() => renameFileWithRetry(source, destination)).toThrow()
    expect(Date.now() - started).toBeLessThan(5_000)
    expect(fs.readFileSync(source, 'utf8')).toBe('preserved')
    expect(fs.existsSync(destination)).toBe(false)
  } finally {
    execFileSync('icacls', [root, '/reset'])
    execFileSync('icacls', [source, '/reset'])
    fs.rmSync(root, { recursive: true })
  }
})

windowsTest('a directory destination fails promptly and preserves its children', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'rename-retry-'))
  const source = path.join(root, 'source')
  const destination = path.join(root, 'destination')
  try {
    fs.writeFileSync(source, 'source')
    fs.mkdirSync(destination)
    fs.writeFileSync(path.join(destination, 'child'), 'preserved')
    const started = Date.now()
    expect(() => renameFileWithRetry(source, destination)).toThrow()
    expect(Date.now() - started).toBeLessThan(5_000)
    expect(fs.readFileSync(source, 'utf8')).toBe('source')
    expect(fs.readFileSync(path.join(destination, 'child'), 'utf8')).toBe('preserved')
  } finally {
    fs.rmSync(root, { recursive: true })
  }
})
