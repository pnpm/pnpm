import path from 'node:path'

import { expect, test } from '@jest/globals'
import { cache } from '@pnpm/cache.commands'

test('print the cache directory', async () => {
  const cacheDir = path.resolve('cache')

  const result = await cache.handler({
    cacheDir,
    cliOptions: {},
    pnpmHomeDir: '',
    registrySupportsTimeField: false,
    resolutionMode: 'highest',
    storeDir: path.resolve('store'),
  }, ['path'])

  expect(result).toBe(cacheDir)
})
