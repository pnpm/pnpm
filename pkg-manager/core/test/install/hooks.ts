import { LOCKFILE_VERSION } from '@pnpm/constants'
import { type LockfileObject } from '@pnpm/lockfile.fs'
import { prepareEmpty } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import {
  addDependenciesToPackage,
  type PackageManifest,
} from '@pnpm/core'
import { testDefaults } from '../utils'

test('readPackage, afterAllResolved hooks', async () => {
  const project = prepareEmpty()

  // w/o the hook, 100.1.0 would be installed
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  function readPackageHook (manifest: PackageManifest) {
    switch (manifest.name) {
    case '@pnpm.e2e/pkg-with-1-dep':
      if (manifest.dependencies == null) {
        throw new Error('@pnpm.e2e/pkg-with-1-dep expected to have a dependencies field')
      }
      manifest.dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep'] = '100.0.0'
      break
    }
    return manifest
  }

  const afterAllResolved = jest.fn((lockfile: LockfileObject) => {
    Object.assign(lockfile, { foo: 'foo' })
    return lockfile
  })

  await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-dep'], testDefaults({
    hooks: {
      afterAllResolved: [afterAllResolved],
      readPackage: [readPackageHook],
    },
  }))

  project.storeHas('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.0.0')
  expect(afterAllResolved).toHaveBeenCalledTimes(1)
  expect(afterAllResolved.mock.calls[0][0].lockfileVersion).toEqual(LOCKFILE_VERSION)

  const wantedLockfile = project.readLockfile()
  expect(wantedLockfile).toHaveProperty(['foo'], 'foo')
})

test('readPackage, afterAllResolved async hooks', async () => {
  const project = prepareEmpty()

  // w/o the hook, 100.1.0 would be installed
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  async function readPackageHook (manifest: PackageManifest) {
    switch (manifest.name) {
    case '@pnpm.e2e/pkg-with-1-dep':
      if (manifest.dependencies == null) {
        throw new Error('@pnpm.e2e/pkg-with-1-dep expected to have a dependencies field')
      }
      manifest.dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep'] = '100.0.0'
      break
    }
    return manifest
  }

  const afterAllResolved = jest.fn(async (lockfile: LockfileObject) => {
    Object.assign(lockfile, { foo: 'foo' })
    return lockfile
  })

  await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-dep'], testDefaults({
    hooks: {
      afterAllResolved: [afterAllResolved],
      readPackage: [readPackageHook],
    },
  }))

  project.storeHas('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.0.0')
  expect(afterAllResolved).toHaveBeenCalledTimes(1)
  expect(afterAllResolved.mock.calls[0][0].lockfileVersion).toEqual(LOCKFILE_VERSION)

  const wantedLockfile = project.readLockfile()
  expect(wantedLockfile).toHaveProperty(['foo'], 'foo')
})

test('readPackage hooks array', async () => {
  const project = prepareEmpty()

  // w/o the hook, 100.1.0 would be installed
  await addDistTag({ package: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '100.1.0', distTag: 'latest' })

  function readPackageHook1 (manifest: PackageManifest) {
    switch (manifest.name) {
    case '@pnpm.e2e/pkg-with-1-dep':
      if (manifest.dependencies == null) {
        throw new Error('@pnpm.e2e/pkg-with-1-dep expected to have a dependencies field')
      }
      manifest.dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep'] = '50.0.0'
      break
    }
    return manifest
  }

  function readPackageHook2 (manifest: PackageManifest) {
    switch (manifest.name) {
    case '@pnpm.e2e/pkg-with-1-dep':
      if (manifest.dependencies == null) {
        throw new Error('@pnpm.e2e/pkg-with-1-dep expected to have a dependencies field')
      }
      manifest.dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep'] = '100.0.0'
      break
    }
    return manifest
  }

  await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-1-dep'], testDefaults({
    hooks: {
      readPackage: [readPackageHook1, readPackageHook2],
    },
  }))

  project.storeHas('@pnpm.e2e/dep-of-pkg-with-1-dep', '100.0.0')
})
