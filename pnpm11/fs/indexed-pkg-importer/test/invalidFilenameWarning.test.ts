import fs from 'node:fs'
import path from 'node:path'

import { expect, jest, test } from '@jest/globals'
import { tempDir } from '@pnpm/prepare'

const globalWarn = jest.fn()
jest.unstable_mockModule('@pnpm/logger', () => ({
  globalInfo: jest.fn(),
  globalWarn,
  logger: () => ({ debug: jest.fn() }),
}))

const { importIndexedDir } = await import('../src/importIndexedDir.js')

test('importIndexedDir() strips control characters from the renamed-file warning', () => {
  const tmp = tempDir()
  const src = path.join(tmp, 'src.txt')
  fs.writeFileSync(src, 'content')
  const importFile = (from: string, to: string): void => {
    if (to.includes('\u001b')) {
      throw Object.assign(new Error(`ENOENT: ${to}`), { code: 'ENOENT' })
    }
    fs.copyFileSync(from, to)
  }

  importIndexedDir({ importFile, importFileAtomic: importFile }, path.join(tmp, 'dest'), new Map([
    ['\u001b[31m-red.txt', src],
  ]), {})

  expect(fs.readFileSync(path.join(tmp, 'dest/[31m-red.txt'), 'utf8')).toBe('content')
  expect(globalWarn).toHaveBeenCalledTimes(1)
  const message = globalWarn.mock.calls[0][0] as string
  expect(message).toContain('[31m-red.txt')
  expect(message).not.toContain('\u001b')
})
