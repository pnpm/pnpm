import { prepareEmpty } from '@pnpm/prepare'
import { fixtures } from '@pnpm/test-fixtures'
import { addDependenciesToPackage } from '@pnpm/core'
import { testDefaults } from '../utils'

const f = fixtures(__dirname)

test('bundledDependencies (pkg-with-bundled-dependencies@1.0.0)', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-bundled-dependencies@1.0.0'], testDefaults({ fastUnpack: false }))

  project.isExecutable('@pnpm.e2e/pkg-with-bundled-dependencies/node_modules/.bin/hello-world-js-bin')

  const lockfile = project.readLockfile()
  expect(
    lockfile.packages['@pnpm.e2e/pkg-with-bundled-dependencies@1.0.0'].bundledDependencies
  ).toStrictEqual(
    ['@pnpm.e2e/hello-world-js-bin']
  )

  expect(
    lockfile.packages['@pnpm.e2e/hello-world-js-bin@1.0.0']
  ).toBeUndefined()
})

// covers https://github.com/pnpm/pnpm/issues/7411
test('local tarball with bundledDependencies', async () => {
  const project = prepareEmpty()

  f.copy('pkg-with-bundled-dependencies-1.0.0.tgz', 'pkg.tgz')
  await addDependenciesToPackage({}, ['file:pkg.tgz'], testDefaults({ fastUnpack: false }))

  const lockfile = project.readLockfile()
  expect(
    lockfile.packages['@pnpm.e2e/pkg-with-bundled-dependencies@file:pkg.tgz'].bundledDependencies
  ).toStrictEqual(
    ['@pnpm.e2e/hello-world-js-bin']
  )
  expect(
    lockfile.packages['@pnpm.e2e/hello-world-js-bin@1.0.0']
  ).toBeUndefined()
})

test('local tarball with bundledDependencies true', async () => {
  const project = prepareEmpty()

  f.copy('pkg-with-bundle-dependencies-true-1.0.0.tgz', 'pkg.tgz')
  await addDependenciesToPackage({}, ['file:pkg.tgz'], testDefaults({ fastUnpack: false }))

  const lockfile = project.readLockfile()
  expect(
    lockfile.packages['@pnpm.e2e/pkg-with-bundle-dependencies-true@file:pkg.tgz'].bundledDependencies
  ).toStrictEqual(
    true
  )
  expect(
    lockfile.packages['@pnpm.e2e/hello-world-js-bin@1.0.0']
  ).toBeUndefined()
})

test('bundleDependencies (pkg-with-bundle-dependencies@1.0.0)', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-bundle-dependencies@1.0.0'], testDefaults({ fastUnpack: false }))

  project.isExecutable('@pnpm.e2e/pkg-with-bundle-dependencies/node_modules/.bin/hello-world-js-bin')

  const lockfile = project.readLockfile()
  expect(
    lockfile.packages['@pnpm.e2e/pkg-with-bundle-dependencies@1.0.0'].bundledDependencies
  ).toStrictEqual(
    ['@pnpm.e2e/hello-world-js-bin']
  )
  expect(
    lockfile.packages['@pnpm.e2e/hello-world-js-bin@1.0.0']
  ).toBeUndefined()
})

test('installing a package with bundleDependencies set to false (pkg-with-bundle-dependencies-false)', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-bundle-dependencies-false'], testDefaults({ fastUnpack: false }))

  const lockfile = project.readLockfile()
  expect(
    typeof lockfile.packages['@pnpm.e2e/pkg-with-bundle-dependencies-false@1.0.0'].bundledDependencies
  ).toEqual('undefined')
})

test('installing a package with bundleDependencies set to true (pkg-with-bundle-dependencies-true)', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-bundle-dependencies-true@1.0.0'], testDefaults({ fastUnpack: false }))

  const lockfile = project.readLockfile()

  expect(
    lockfile.packages['@pnpm.e2e/hello-world-js-bin@1.0.0']
  ).toBeUndefined()
})
