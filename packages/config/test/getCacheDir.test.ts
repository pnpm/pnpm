import os from 'os'
import path from 'path'
import getCacheDir from '../lib/getCacheDir'

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
