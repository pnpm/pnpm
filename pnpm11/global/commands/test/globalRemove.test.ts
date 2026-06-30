import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { expect, jest, test } from '@jest/globals'

const removeBin = jest.fn<(cmd: string) => Promise<void>>().mockResolvedValue(undefined)

jest.unstable_mockModule('@pnpm/bins.remover', () => ({ removeBin }))

const { handleGlobalRemove } = await import('../src/globalRemove.js')

// A malicious global package whose manifest declares reserved bin keys must not
// reach the deletion sink: `path.join(globalBinDir, '.')` is the bin directory
// itself and `path.join(globalBinDir, '..')` is its parent, so removing either
// would wipe out unrelated files. Only the safe `good` shim may be deleted.
test('global remove ignores reserved manifest bin names', async () => {
  const globalDir = fs.mkdtempSync(path.join(os.tmpdir(), 'global-remove-'))
  const globalBinDir = path.join(globalDir, 'bin')
  const installDir = path.join(globalDir, 'install')
  const depDir = path.join(installDir, 'node_modules', 'evil')
  fs.mkdirSync(depDir, { recursive: true })
  fs.writeFileSync(
    path.join(installDir, 'package.json'),
    JSON.stringify({ name: 'global', version: '1.0.0', dependencies: { evil: '1.0.0' } })
  )
  fs.writeFileSync(
    path.join(depDir, 'package.json'),
    JSON.stringify({
      name: 'evil',
      version: '1.0.0',
      bin: {
        '': './empty.js',
        '.': './dot.js',
        '..': './dot-dot.js',
        '@scope/..': './scoped-dot-dot.js',
        good: './good.js',
      },
    })
  )
  fs.symlinkSync(installDir, path.join(globalDir, 'hash'))

  await handleGlobalRemove({ globalPkgDir: globalDir, bin: globalBinDir }, ['evil'])

  expect(removeBin).toHaveBeenCalledTimes(1)
  expect(removeBin).toHaveBeenCalledWith(path.join(globalBinDir, 'good'))
})
