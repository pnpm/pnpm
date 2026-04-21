/// <reference path="../../../__typings__/index.d.ts"/>
import fs from 'node:fs'
import { homedir } from 'node:os'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { getConfig } from '@pnpm/config.reader'
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
