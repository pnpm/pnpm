// cspell:ignore WDAC
import { execFileSync, spawn } from 'node:child_process'
import { once } from 'node:events'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { afterEach, expect, jest, test } from '@jest/globals'
import { renameFileWithRetry } from '@pnpm/fs.graceful-fs'

const platform = Object.getOwnPropertyDescriptor(process, 'platform')!
const privilegesScript = path.join(import.meta.dirname, 'processPrivileges.ps1')

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
    // Elevated Windows runners can bypass ACLs with backup/restore privileges.
    const args = ['-NoProfile', '-NonInteractive', '-File', privilegesScript, String(process.pid)]
    const privileges = execFileSync('powershell.exe', args, { encoding: 'utf8' }).trim()
    try {
      expect(() => fs.renameSync(source, destination)).toThrow()
      const started = Date.now()
      expect(() => renameFileWithRetry(source, destination)).toThrow()
      expect(Date.now() - started).toBeLessThan(5_000)
      expect(fs.readFileSync(source, 'utf8')).toBe('preserved')
      expect(fs.existsSync(destination)).toBe(false)
    } finally {
      execFileSync('powershell.exe', [...args, privileges])
    }
  } finally {
    execFileSync('icacls', [root, '/reset', '/T'])
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

windowsTest('rename recovers after a child process releases a real directory lock', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'rename-retry-'))
  const source = path.join(root, 'source')
  const destination = path.join(root, 'destination')
  const lockedFile = path.join(source, 'child')
  const releaseFile = path.join(root, 'release')
  fs.mkdirSync(source)
  fs.writeFileSync(lockedFile, 'preserved')
  const child = spawn('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', `
    $ErrorActionPreference = 'Stop'
    $handle = [System.IO.File]::Open($env:PNPM_TEST_LOCK_FILE, 'Open', 'Read', 'ReadWrite')
    try {
      Write-Output 'ready'
      $deadline = [DateTime]::UtcNow.AddSeconds(10)
      while (-not (Test-Path -LiteralPath $env:PNPM_TEST_RELEASE_FILE)) {
        if ([DateTime]::UtcNow -ge $deadline) { throw 'Lock release timed out' }
        Start-Sleep -Milliseconds 10
      }
    } finally {
      $handle.Dispose()
    }
  `], {
    env: { ...process.env, PNPM_TEST_LOCK_FILE: lockedFile, PNPM_TEST_RELEASE_FILE: releaseFile },
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  let stderr = ''
  child.stderr.on('data', (chunk) => {
    stderr += String(chunk)
  })
  const exited = new Promise<number | Error | null>((resolve) => {
    child.once('error', resolve)
    child.once('exit', resolve)
  })
  try {
    await Promise.race([
      once(child.stdout, 'data').then(([chunk]) => {
        expect(String(chunk).trim()).toBe('ready')
      }),
      exited.then((code) => {
        throw new Error(`Lock holder exited early (${code}): ${stderr}`)
      }),
    ])
    const rename = fs.renameSync
    const failures: unknown[] = []
    jest.spyOn(fs, 'renameSync').mockImplementation((from, to) => {
      try {
        rename(from, to)
      } catch (error) {
        failures.push(error)
        fs.writeFileSync(releaseFile, '')
        throw error
      }
    })
    renameFileWithRetry(source, destination)
    expect(failures.length).toBeGreaterThan(0)
    expect(failures[0]).toMatchObject({ code: 'EPERM' })
    expect(await exited).toBe(0)
    expect(fs.readFileSync(path.join(destination, 'child'), 'utf8')).toBe('preserved')
  } finally {
    child.kill()
    await exited
    fs.rmSync(root, { recursive: true })
  }
})
