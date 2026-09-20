import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import { addDependenciesToPackage, install } from '@pnpm/installing.deps-installer'
import { prepareEmpty } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/testing.registry-mock'
import { rimrafSync } from '@zkochan/rimraf'
import { loadJsonFileSync } from 'load-json-file'

import { testDefaults } from '../utils/index.js'

test.each(['3.1.0', '1.0.0'])('adding a new unnamed tarball URL replaces the existing dependency with version %s', async (newVersion) => {
  const project = prepareEmpty()
  const tarballUrl = (version: string) => `http://127.0.0.1:${REGISTRY_MOCK_PORT}/is-positive/-/is-positive-${version}.tgz`
  const oldTarball = tarballUrl('1.0.0')
  const newTarball = `${tarballUrl(newVersion)}?revision=2`
  const { updatedManifest } = await addDependenciesToPackage({}, [oldTarball], testDefaults())
  expect(loadJsonFileSync<{ version: string }>('node_modules/is-positive/package.json').version).toBe('1.0.0')
  const oldPackageDir = fs.realpathSync('node_modules/is-positive')
  const result = await addDependenciesToPackage(updatedManifest, [newTarball], testDefaults())

  expect(result.updatedManifest.dependencies?.['is-positive']).toBe(newTarball)
  expect(loadJsonFileSync<{ version: string }>('node_modules/is-positive/package.json').version).toBe(newVersion)
  expect(fs.realpathSync('node_modules/is-positive')).not.toBe(oldPackageDir)
  const lockfile = project.readLockfile()
  expect(lockfile.importers['.'].dependencies?.['is-positive']).toEqual({
    specifier: newTarball,
    version: newTarball,
  })
  expect(lockfile.packages).toHaveProperty([`is-positive@${newTarball}`])
  expect(lockfile.packages).not.toHaveProperty([`is-positive@${oldTarball}`])

  rimrafSync('node_modules')
  await install(result.updatedManifest, testDefaults({ frozenLockfile: true }))
  expect(loadJsonFileSync<{ version: string }>('node_modules/is-positive/package.json').version).toBe(newVersion)
  expect(project.readLockfile()).toEqual(lockfile)
})
