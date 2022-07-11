/// <reference path="../../../typings/index.d.ts"/>
import fs from 'fs'
import { tempDir } from '@pnpm/prepare'
import path from 'path'
import pathName from 'path-name'
import symlinkDir from 'symlink-dir'
import { homedir } from 'os'
import getConfig from '@pnpm/config'

const globalBinDir = path.join(homedir(), '.local', 'pnpm')
const isWindows = process.platform === 'win32'

jest.mock('@pnpm/npm-conf/lib/conf', () => {
  const originalModule = jest.requireActual('@pnpm/npm-conf/lib/conf')
  class MockedConf extends originalModule {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    constructor (base: any, types: any) {
      super(base, types)
      const globalPrefixDirName = isWindows ? 'global-bin-dir-windows' : 'global-bin-dir'
      this.prefix = this.globalPrefix = path.join(__dirname, globalPrefixDirName)
      this.localPrefix = __dirname
    }

    get (name: string) {
      if (name === 'prefix') {
        return this.prefix
      } else {
        return super.get(name)
      }
    }

    loadPrefix () {}
  }
  return MockedConf
})

test('respects global-bin-dir in npmrc', async () => {
  const { config } = await getConfig({
    cliOptions: {
      global: true,
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
      dir: __dirname,
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
  await expect(
    getConfig({
      cliOptions: {
        global: true,
        dir: __dirname,
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
      dir: __dirname,
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
