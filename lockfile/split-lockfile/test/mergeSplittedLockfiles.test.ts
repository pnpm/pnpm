import { Lockfile } from '@pnpm/lockfile-types'
import { isEmpty } from 'ramda'
import { mergeSplittedLockfiles, splitLockfile } from '@pnpm/split-lockfile'
import path from 'path'
import { loadLockfile } from './utils'
import { writeWantedLockfile } from '@pnpm/lockfile-file'
import fs from 'fs'
import tempy from 'tempy'

function isEmptyLockfile (lockfile: Lockfile) {
  return Object.values(lockfile.importers).every((importer) => isEmpty(importer.specifiers ?? {}) && isEmpty(importer.dependencies ?? {}))
}

test('mergeLockfile() should works for empty', async () => {
  const lockfile = mergeSplittedLockfiles({})
  expect(isEmptyLockfile(lockfile)).toBeTruthy()
})

test('mergeLockfile() should throw Error when it doesn\'t container root lockfile', async () => {
  const lockfile = {
    a: {
      importers: {},
      lockfileVersion: '5.4',
    },
  }
  expect(() => mergeSplittedLockfiles(lockfile)).toThrowError()
})

function resolveLockfilePath (dir: string): string {
  return path.resolve(dir, 'pnpm-lock.yaml')
}

describe('mergeLockfile should generate same data', () => {
  const fixture = path.resolve(__dirname, './fixture')
  const list = fs.readdirSync(fixture)
  list.forEach((item) => {
    const pkgPath = path.resolve(fixture, item)
    test(`${pkgPath} should works`, async () => {
      const lockfile = await loadLockfile(pkgPath)
      const splitted = splitLockfile(lockfile)
      const merged = mergeSplittedLockfiles(splitted)
      const tempPath = tempy.directory()
      await writeWantedLockfile(tempPath, merged)
      const raw = (await fs.promises.readFile(resolveLockfilePath(pkgPath))).toString('utf-8')
      const actual = (await fs.promises.readFile(resolveLockfilePath(tempPath))).toString('utf-8')
      expect(actual).toBe(raw)
    })
  })
})
