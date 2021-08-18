import os from 'os'
import path from 'path'
import { getCacheDir, getDataDir, getStateDir } from '../lib/dirs'

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
    env: {},
    platform: 'darwin',
  })).toBe(path.join(os.homedir(), 'Library/Caches/pnpm'))
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
    env: {},
    platform: 'darwin',
  })).toBe(path.join(os.homedir(), '.pnpm-state'))
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

test('getDataDir()', () => {
  expect(getDataDir({
    env: {
      XDG_DATA_HOME: '/home/foo/data',
    },
    platform: 'linux',
  })).toBe(path.join('/home/foo/data', 'pnpm'))
  expect(getDataDir({
    env: {},
    platform: 'linux',
  })).toBe(path.join(os.homedir(), '.local/share/pnpm'))
  expect(getDataDir({
    env: {},
    platform: 'darwin',
  })).toBe(path.join(os.homedir(), 'Library/pnpm'))
  expect(getDataDir({
    env: {
      LOCALAPPDATA: '/localappdata',
    },
    platform: 'win32',
  })).toBe(path.join('/localappdata', 'pnpm'))
  expect(getDataDir({
    env: {},
    platform: 'win32',
  })).toBe(path.join(os.homedir(), '.pnpm'))
})
