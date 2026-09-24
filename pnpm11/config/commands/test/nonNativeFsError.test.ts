import path from 'node:path'

import { expect, jest, test } from '@jest/globals'
import { tempDir } from '@pnpm/prepare'

// StackBlitz WebContainers throw fs errors that carry the usual errno fields
// but are not native errors (util.types.isNativeError() returns false).
function toNonNativeError (err: unknown): unknown {
  if (err == null || typeof err !== 'object') return err
  return Object.assign(Object.create(Error.prototype) as Error, err, { message: (err as Error).message })
}

const originalReadIniFile = await import('read-ini-file')

jest.unstable_mockModule('read-ini-file', () => ({
  ...originalReadIniFile,
  readIniFile: async (filePath: string) => {
    try {
      return await originalReadIniFile.readIniFile(filePath)
    } catch (err) {
      throw toNonNativeError(err)
    }
  },
}))

const { config } = await import('@pnpm/config.commands')
const { readIniFileSync } = await import('read-ini-file')
const { createConfigCommandOpts } = await import('./utils/index.js')

test('config set creates a missing auth.ini when the fs error is not a native error', async () => {
  const tmp = tempDir()
  const configDir = path.join(tmp, 'global-config')

  await config.handler(createConfigCommandOpts({
    dir: tmp,
    cliOptions: {},
    configDir,
    global: true,
    authConfig: {},
  }), ['set', 'registry', 'https://npm-registry.example.com/'])

  expect(readIniFileSync(path.join(configDir, 'auth.ini'))).toEqual({
    registry: 'https://npm-registry.example.com/',
  })
})
