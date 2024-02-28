import type { ProjectManifest } from '@pnpm/types'
import { prepareEmpty } from '@pnpm/prepare'
import { addDependenciesToPackage, install } from '@pnpm/core'
import {
  testDefaults,
} from '../utils'

test('ignoredOptionalDependencies causes listed optional dependencies to be skipped', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage(
    {},
    ['@pnpm.e2e/pkg-with-good-optional@1.0.0'],
    testDefaults({ ignoredOptionalDependencies: ['is-positive'] })
  )

  const lockfile = project.readLockfile()
  expect(lockfile.ignoredOptionalDependencies).toStrictEqual(['is-positive'])
  expect(lockfile.packages).not.toHaveProperty(['/is-positive@1.0.0'])
  expect(lockfile.packages).toHaveProperty(['/@pnpm.e2e/pkg-with-good-optional@1.0.0'])
})

test('empty ignoredOptionalDependencies is not recorded in lockfile', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage(
    {},
    ['@pnpm.e2e/pkg-with-good-optional@1.0.0'],
    testDefaults({ ignoredOptionalDependencies: [] })
  )

  const lockfile = project.readLockfile()
  expect(lockfile).not.toHaveProperty(['ignoredOptionalDependencies'])
  expect(lockfile.packages).toHaveProperty(['/is-positive@1.0.0'])
  expect(lockfile.packages).toHaveProperty(['/@pnpm.e2e/pkg-with-good-optional@1.0.0'])
})

test('names in ignoredOptionalDependencies are sorted alphabetically in the lockfile', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage(
    {},
    ['@pnpm.e2e/pkg-with-good-optional@1.0.0'],
    testDefaults({ ignoredOptionalDependencies: ['foo', 'bar', 'baz', 'qux'] })
  )

  const lockfile = project.readLockfile()
  expect(lockfile.ignoredOptionalDependencies).toStrictEqual(['bar', 'baz', 'foo', 'qux'])
})

test('adding or changing manifest.pnpm.ignoredOptionalDependencies should change lockfile.ignoredOptionalDependencies and module structure', async () => {
  const manifest: ProjectManifest = Object.freeze({
    dependencies: {
      '@pnpm.e2e/pkg-with-good-optional': '1.0.0',
    },
  })
  const project = prepareEmpty()

  await install(manifest, testDefaults())
  {
    const lockfile = project.readLockfile()
    expect(lockfile).not.toHaveProperty(['ignoredOptionalDependencies'])
    expect(lockfile.packages).toHaveProperty(['/@pnpm.e2e/pkg-with-good-optional@1.0.0'])
    expect(lockfile.packages).toHaveProperty(['/is-positive@1.0.0'])
  }

  await install(manifest, testDefaults({
    ignoredOptionalDependencies: ['is-positive'],
  }))
  {
    const lockfile = project.readLockfile()
    expect(lockfile.ignoredOptionalDependencies).toStrictEqual(['is-positive'])
    expect(lockfile.packages).toHaveProperty(['/@pnpm.e2e/pkg-with-good-optional@1.0.0'])
    expect(lockfile.packages).not.toHaveProperty(['/is-positive@1.0.0'])
  }
})
