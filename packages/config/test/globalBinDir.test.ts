/// <reference path="../../../typings/index.d.ts"/>
import path from 'path'
import { homedir } from 'os'
import getConfig from '@pnpm/config'

jest.mock('@zkochan/npm-conf/lib/conf', () => {
  const originalModule = jest.requireActual('@zkochan/npm-conf/lib/conf')
  class MockedConf extends originalModule {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    constructor (base: any, types: any) {
      super(base, types)
      this.prefix = this.globalPrefix = path.join(__dirname, 'global-bin-dir')
      this.localPrefix = __dirname
    }

    get (name: string) {
      if (name === 'prefix') {
        return this.globalPrefix
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
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  expect(config.bin).toBe(path.join(homedir(), '.local', 'pnpm'))
})

test('respects global-bin-dir rather than dir', async () => {
  const { config } = await getConfig({
    cliOptions: {
      global: true,
    },
    packageManager: {
      name: 'pnpm',
      version: '1.0.0',
    },
  })
  expect(config.bin).toBe(path.join(homedir(), '.local', 'pnpm'))
})
