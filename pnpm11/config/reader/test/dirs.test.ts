import os from 'node:os'
import path from 'node:path'

import { expect, test } from '@jest/globals'

import { getCacheDir, getConfigDir, getDataDir, getStateDir } from '../lib/dirs.js'

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

test('getDataDir() expands nested Windows environment variables', () => {
  expect(getDataDir({
    env: {
      PNPM_HOME: '%SOME_ENV%/pnpm',
      SOME_ENV: 'C:\\tools',
    },
    platform: 'win32',
  })).toBe('C:\\tools/pnpm')
  expect(getDataDir({
    env: {
      PNPM_HOME: '%OUTER%/pnpm',
      OUTER: '%INNER%\\apps',
      INNER: 'D:\\dev',
    },
    platform: 'win32',
  })).toBe('D:\\dev\\apps/pnpm')
  expect(getDataDir({
    env: {
      PNPM_HOME: '%some_env%/pnpm',
      Some_Env: 'C:\\tools',
    },
    platform: 'win32',
  })).toBe('C:\\tools/pnpm')
})

test('getDataDir() rejects an unexpanded Windows environment variable', () => {
  expect(() => getDataDir({
    env: {
      PNPM_HOME: '%MISSING%/pnpm',
    },
    platform: 'win32',
  })).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_UNEXPANDED_ENV_IN_PATH',
    message: 'PNPM_HOME contains an unexpanded environment variable: %MISSING%',
  }))
  expect(() => getDataDir({
    env: {
      PNPM_HOME: '%A%',
      A: '%B%',
      B: '%A%',
    },
    platform: 'win32',
  })).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_UNEXPANDED_ENV_IN_PATH',
  }))
})

test('getDataDir() leaves percent references unchanged off Windows', () => {
  expect(getDataDir({
    env: {
      PNPM_HOME: '%SOME_ENV%/pnpm',
      SOME_ENV: '/opt/tools',
    },
    platform: 'linux',
  })).toBe('%SOME_ENV%/pnpm')
})

test('getDataDir() leaves a literal percent in place on Windows', () => {
  expect(getDataDir({
    env: {
      PNPM_HOME: 'C:\\100%\\pnpm',
    },
    platform: 'win32',
  })).toBe('C:\\100%\\pnpm')
})

test('getCacheDir() expands LOCALAPPDATA and rejects an unexpanded reference', () => {
  expect(getCacheDir({
    env: {
      LOCALAPPDATA: '%LOCAL_ROOT%\\AppData\\Local',
      LOCAL_ROOT: 'C:\\Users\\me',
    },
    platform: 'win32',
  })).toBe(path.join('C:\\Users\\me\\AppData\\Local', 'pnpm-cache'))
  expect(() => getCacheDir({
    env: {
      LOCALAPPDATA: '%MISSING%\\AppData\\Local',
    },
    platform: 'win32',
  })).toThrow(expect.objectContaining({
    code: 'ERR_PNPM_UNEXPANDED_ENV_IN_PATH',
    message: 'LOCALAPPDATA contains an unexpanded environment variable: %MISSING%',
  }))
})

test('getDataDir()', () => {
  expect(getDataDir({
    env: {
      PNPM_HOME: '/home/foo/data',
    },
    platform: 'linux',
  })).toBe('/home/foo/data')
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

test('getConfigDir()', () => {
  expect(getConfigDir({
    env: {
      XDG_CONFIG_HOME: '/home/foo/config',
    },
    platform: 'linux',
  })).toBe(path.join('/home/foo/config', 'pnpm'))
  expect(getConfigDir({
    env: {},
    platform: 'linux',
  })).toBe(path.join(os.homedir(), '.config/pnpm'))
  expect(getConfigDir({
    env: {},
    platform: 'darwin',
  })).toBe(path.join(os.homedir(), 'Library/Preferences/pnpm'))
  expect(getConfigDir({
    env: {
      LOCALAPPDATA: '/localappdata',
    },
    platform: 'win32',
  })).toBe(path.join('/localappdata', 'pnpm/config'))
  expect(getConfigDir({
    env: {},
    platform: 'win32',
  })).toBe(path.join(os.homedir(), '.config/pnpm'))
})
