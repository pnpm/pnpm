import os from 'os'
import path from 'path'
import { getCacheDir, getStateDir } from '../lib/dirs'

test('getCacheDir()', () => {
  expect(getCacheDir({
    env: {
      XDG_CACHE_HOME: '/home/foo/cache',
    },
    platform: 'linux',
  })).toBe(path.join('/home/foo/cache', 'pnpm'))
  expect(getCacheDir({
    env: {},
    platform: 'linux',
  })).toBe(path.join(os.homedir(), '.cache/pnpm'))
  expect(getCacheDir({
    env: {
      LOCALAPPDATA: '/localappdata',
    },
    platform: 'win32',
  })).toBe(path.join('/localappdata', 'pnpm-cache'))
  expect(getCacheDir({
    env: {},
    platform: 'win32',
  })).toBe(path.join(os.homedir(), '.pnpm-cache'))
})

test('getStateDir()', () => {
  expect(getStateDir({
    env: {
      XDG_STATE_HOME: '/home/foo/state',
    },
    platform: 'linux',
  })).toBe(path.join('/home/foo/state', 'pnpm'))
  expect(getStateDir({
    env: {},
    platform: 'linux',
  })).toBe(path.join(os.homedir(), '.local/state/pnpm'))
  expect(getStateDir({
    env: {
      LOCALAPPDATA: '/localappdata',
    },
    platform: 'win32',
  })).toBe(path.join('/localappdata', 'pnpm-state'))
  expect(getStateDir({
    env: {},
    platform: 'win32',
  })).toBe(path.join(os.homedir(), '.pnpm-state'))
})
