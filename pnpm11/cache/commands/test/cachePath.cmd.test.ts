import path from 'node:path'

import { expect, test } from '@jest/globals'
import { cache } from '@pnpm/cache.commands'

test('print the cache directory', async () => {
  const cacheDir = path.resolve('cache')
  // The printed path is handed to other tools, so it is cleaned of `.` and
  // `..` segments rather than echoed back as configured.
  const configuredCacheDir = [cacheDir, '..', '.', 'cache'].join(path.sep)

  const result = await cache.handler({
    cacheDir: configuredCacheDir,
    cliOptions: {},
    pnpmHomeDir: '',
    registrySupportsTimeField: false,
    resolutionMode: 'highest',
    storeDir: path.resolve('store'),
  }, ['path'])

  expect(result).toBe(cacheDir)
})
