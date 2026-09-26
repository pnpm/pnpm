/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'node:fs'
import { homedir } from 'node:os'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { getConfig } from '@pnpm/config.reader'
import { GLOBAL_LAYOUT_VERSION } from '@pnpm/constants'
import { tempDir } from '@pnpm/prepare'
import pathName from 'path-name'
import { symlinkDir } from 'symlink-dir'

const globalBinDir = path.join(homedir(), '.local', 'pnpm')

test('respects global-bin-dir from CLI', async () => {
  const { config } = await getConfig({
    cliOptions: {
      global: true,
      'global-bin-dir': globalBinDir,
    },
    env: {
      [pathName]: `${globalBinDir}${path.delimiter}${process.env[pathName]!}`,
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  expect(config.bin).toBe(globalBinDir)
})

test('respects global-bin-dir rather than dir', async () => {
  const { config } = await getConfig({
    cliOptions: {
      global: true,
      'global-bin-dir': globalBinDir,
      dir: import.meta.dirname,
    },
    env: {
      [pathName]: `${globalBinDir}${path.delimiter}${process.env[pathName]!}`,
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  expect(config.bin).toBe(globalBinDir)
})

test('an exception is thrown when the global dir is not in PATH', async () => {
  const tmp = tempDir()
  const binDir = path.join(tmp, 'not-in-path-bin')
  fs.mkdirSync(binDir, { recursive: true })
  await expect(
    getConfig({
      cliOptions: {
        global: true,
        'global-bin-dir': binDir,
        dir: import.meta.dirname,
      },
      env: {
        [pathName]: process.env[pathName],
      },
      packageManager: {
        name: 'pnpm',
        version: '1.0.0',
      },
    })
  ).rejects.toThrow(/is not in PATH/)
})

test('the global directory may be a symlink to a directory that is in PATH', async () => {
  const tmp = tempDir()
  const globalBinDirTarget = path.join(tmp, 'global-target')
  fs.mkdirSync(globalBinDirTarget)
  const globalBinDirSymlink = path.join(tmp, 'global-symlink')
  await symlinkDir(globalBinDirTarget, globalBinDirSymlink)
  const { config } = await getConfig({
    cliOptions: {
      global: true,
      'global-bin-dir': globalBinDirSymlink,
      dir: import.meta.dirname,
    },
    env: {
      [pathName]: `${globalBinDirTarget}${path.delimiter}${process.env[pathName]!}`,
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  expect(config.bin).toBe(globalBinDirSymlink)
})

test('a leading ~ is expanded before the global directories are derived', async () => {
  const { config } = await getConfig({
    cliOptions: {
      global: true,
      'global-bin-dir': '~/.local/pnpm',
      'global-dir': '~/.local/share/pnpm-global',
    },
    env: {
      [pathName]: `${globalBinDir}${path.delimiter}${process.env[pathName]!}`,
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  expect(config.bin).toBe(globalBinDir)
  expect(config.globalPkgDir).toBe(path.join(homedir(), '.local', 'share', 'pnpm-global', GLOBAL_LAYOUT_VERSION))
})

test('PNPM_CONFIG_GLOBAL_BIN_DIR and PNPM_CONFIG_GLOBAL_DIR reach the derived directories', async () => {
  const tmp = tempDir()
  const envBinDir = path.join(tmp, 'env-bin')
  fs.mkdirSync(envBinDir, { recursive: true })
  const { config } = await getConfig({
    cliOptions: {
      global: true,
      dir: import.meta.dirname,
    },
    env: {
      [pathName]: `${envBinDir}${path.delimiter}${process.env[pathName]!}`,
      PNPM_CONFIG_GLOBAL_BIN_DIR: envBinDir,
      PNPM_CONFIG_GLOBAL_DIR: path.join(tmp, 'env-global'),
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  expect(config.bin).toBe(envBinDir)
  expect(config.globalPkgDir).toBe(path.join(tmp, 'env-global', GLOBAL_LAYOUT_VERSION))
})

test('a global-bin-dir on the command line outranks PNPM_CONFIG_GLOBAL_BIN_DIR', async () => {
  const tmp = tempDir()
  const cliBinDir = path.join(tmp, 'cli-bin')
  fs.mkdirSync(cliBinDir, { recursive: true })
  const { config } = await getConfig({
    cliOptions: {
      global: true,
      'global-bin-dir': cliBinDir,
      dir: import.meta.dirname,
    },
    env: {
      [pathName]: `${cliBinDir}${path.delimiter}${process.env[pathName]!}`,
      PNPM_CONFIG_GLOBAL_BIN_DIR: path.join(tmp, 'env-bin'),
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  expect(config.bin).toBe(cliBinDir)
})
