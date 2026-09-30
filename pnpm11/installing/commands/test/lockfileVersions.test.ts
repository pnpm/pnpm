import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { temporaryDirectory } from 'tempy'

import { readVersionsByPackageNames } from '../src/import/lockfileVersions.js'

test('reads the versions of a package whose name is an Object.prototype key', async () => {
  const dir = temporaryDirectory()
  fs.writeFileSync(path.join(dir, 'package-lock.json'), JSON.stringify({
    lockfileVersion: 1,
    dependencies: {
      constructor: { version: '1.0.0' },
    },
  }))

  const versionsByPackageNames = await readVersionsByPackageNames(dir)

  expect(Array.from(versionsByPackageNames['constructor'])).toStrictEqual(['1.0.0'])
})
