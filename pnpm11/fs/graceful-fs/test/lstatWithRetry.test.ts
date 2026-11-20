import fs from 'node:fs'

import { afterEach, expect, jest, test } from '@jest/globals'
import { lstatWithRetry, unlinkWithRetry } from '@pnpm/fs.graceful-fs'

const platform = Object.getOwnPropertyDescriptor(process, 'platform')!

afterEach(() => {
  jest.restoreAllMocks()
  Object.defineProperty(process, 'platform', platform)
})

test('a refusal that resolves into absence surfaces the absence', () => {
  onWindowsWithInstantBackoff()
  const absent = Object.assign(new Error('no such file'), { code: 'ENOENT' })
  let attempts = 0
  jest.spyOn(fs, 'lstatSync').mockImplementation(() => {
    attempts++
    if (attempts < 3) throw Object.assign(new Error('access denied'), { code: 'EPERM' })
    throw absent
  })

  expect(() => lstatWithRetry('target')).toThrow(absent)
  expect(attempts).toBe(3)
})

test('a refusal that clears surfaces the stats behind it', () => {
  onWindowsWithInstantBackoff()
  const stats = { isDirectory: () => false } as fs.Stats
  let attempts = 0
  jest.spyOn(fs, 'lstatSync').mockImplementation(() => {
    attempts++
    if (attempts < 2) throw Object.assign(new Error('access denied'), { code: 'EPERM' })
    return stats
  })

  expect(lstatWithRetry('target')).toBe(stats)
  expect(attempts).toBe(2)
})

test('an absent entry is reported without retrying', () => {
  onWindowsWithInstantBackoff()
  const absent = Object.assign(new Error('no such file'), { code: 'ENOENT' })
  let attempts = 0
  jest.spyOn(fs, 'lstatSync').mockImplementation(() => {
    attempts++
    throw absent
  })

  expect(() => lstatWithRetry('target')).toThrow(absent)
  expect(attempts).toBe(1)
})

test('a refusal is final off Windows, where it means a permanent problem', () => {
  Object.defineProperty(process, 'platform', { value: 'linux' })
  const denied = Object.assign(new Error('access denied'), { code: 'EACCES' })
  let attempts = 0
  jest.spyOn(fs, 'lstatSync').mockImplementation(() => {
    attempts++
    throw denied
  })

  expect(() => lstatWithRetry('target')).toThrow(denied)
  expect(attempts).toBe(1)
})

test('a refusal to unlink that clears lets the removal through', () => {
  onWindowsWithInstantBackoff()
  let attempts = 0
  jest.spyOn(fs, 'unlinkSync').mockImplementation(() => {
    attempts++
    if (attempts < 2) throw Object.assign(new Error('access denied'), { code: 'EPERM' })
  })

  unlinkWithRetry('target')

  expect(attempts).toBe(2)
})

function onWindowsWithInstantBackoff (): void {
  Object.defineProperty(process, 'platform', { value: 'win32' })
  jest.spyOn(Atomics, 'wait').mockReturnValue('timed-out')
}
