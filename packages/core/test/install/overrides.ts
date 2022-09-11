import PnpmError from '@pnpm/error'
import { prepareEmpty } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import { addDependenciesToPackage, mutateModulesInSingleProject } from '@pnpm/core'
import {
  testDefaults,
} from '../utils'

test('versions are replaced with versions specified through overrides option', async () => {
  const project = prepareEmpty()

  await addDistTag({ package: '@pnpm.e2e/bar', version: '100.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.0.0', distTag: 'latest' })

  const overrides = {
    '@pnpm.e2e/foobarqar>@pnpm.e2e/foo': 'npm:@pnpm.e2e/qar@100.0.0',
    '@pnpm.e2e/bar@^100.0.0': '100.1.0',
    '@pnpm.e2e/dep-of-pkg-with-1-dep': '101.0.0',
  }
  const manifest = await addDependenciesToPackage({},
    ['@pnpm.e2e/pkg-with-1-dep@100.0.0', '@pnpm.e2e/foobar@100.0.0', '@pnpm.e2e/foobarqar@1.0.0'],
    await testDefaults({ overrides })
  )

  {
    const lockfile = await project.readLockfile()
    expect(lockfile.packages['/@pnpm.e2e/foobarqar/1.0.0'].dependencies?.['@pnpm.e2e/foo']).toBe('/@pnpm.e2e/qar/100.0.0')
    expect(lockfile.packages['/@pnpm.e2e/foobar/100.0.0'].dependencies?.['@pnpm.e2e/foo']).toBe('100.0.0')
    expect(lockfile.packages).toHaveProperty(['/@pnpm.e2e/dep-of-pkg-with-1-dep/101.0.0'])
    expect(lockfile.packages).toHaveProperty(['/@pnpm.e2e/bar/100.1.0'])
    expect(lockfile.overrides).toStrictEqual({
      '@pnpm.e2e/foobarqar>@pnpm.e2e/foo': 'npm:@pnpm.e2e/qar@100.0.0',
      '@pnpm.e2e/bar@^100.0.0': '100.1.0',
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '101.0.0',
    })
    const currentLockfile = await project.readCurrentLockfile()
    expect(lockfile.overrides).toStrictEqual(currentLockfile.overrides)
  }
  // shall be able to install when package manifest is ignored
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd(),
  }, { ...await testDefaults(), ignorePackageManifest: true, overrides })

  // The lockfile is updated if the overrides are changed
  overrides['@pnpm.e2e/bar@^100.0.0'] = '100.0.0'
  // A direct dependency may be overriden as well
  overrides['@pnpm.e2e/foobarqar'] = '1.0.1'
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd(),
  }, await testDefaults({ overrides }))

  {
    const lockfile = await project.readLockfile()
    expect(lockfile.packages).toHaveProperty(['/@pnpm.e2e/dep-of-pkg-with-1-dep/101.0.0'])
    expect(lockfile.packages).toHaveProperty(['/@pnpm.e2e/bar/100.0.0'])
    expect(lockfile.packages).toHaveProperty(['/@pnpm.e2e/foobarqar/1.0.1'])
    expect(lockfile.overrides).toStrictEqual({
      '@pnpm.e2e/foobarqar': '1.0.1',
      '@pnpm.e2e/foobarqar>@pnpm.e2e/foo': 'npm:@pnpm.e2e/qar@100.0.0',
      '@pnpm.e2e/bar@^100.0.0': '100.0.0',
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '101.0.0',
    })
    const currentLockfile = await project.readCurrentLockfile()
    expect(lockfile.overrides).toStrictEqual(currentLockfile.overrides)
  }

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd(),
  }, await testDefaults({ frozenLockfile: true, overrides }))

  {
    const lockfile = await project.readLockfile()
    expect(lockfile.overrides).toStrictEqual({
      '@pnpm.e2e/foobarqar': '1.0.1',
      '@pnpm.e2e/foobarqar>@pnpm.e2e/foo': 'npm:@pnpm.e2e/qar@100.0.0',
      '@pnpm.e2e/bar@^100.0.0': '100.0.0',
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '101.0.0',
    })
    const currentLockfile = await project.readCurrentLockfile()
    expect(lockfile.overrides).toStrictEqual(currentLockfile.overrides)
  }

  overrides['@pnpm.e2e/bar@^100.0.0'] = '100.0.1'
  await expect(
    mutateModulesInSingleProject({
      manifest,
      mutation: 'install',
      rootDir: process.cwd(),
    }, await testDefaults({ frozenLockfile: true, overrides }))
  ).rejects.toThrow(
    new PnpmError('FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE',
      'Cannot perform a frozen installation because the lockfile needs updates'
    )
  )
})

test('when adding a new dependency that is present in the overrides, use the spec from the override', async () => {
  prepareEmpty()

  await addDistTag({ package: '@pnpm.e2e/bar', version: '100.0.0', distTag: 'latest' })

  const overrides = {
    '@pnpm.e2e/bar': '100.1.0',
  }
  const manifest = await addDependenciesToPackage({},
    ['@pnpm.e2e/bar'],
    await testDefaults({ overrides })
  )

  expect(manifest.dependencies?.['@pnpm.e2e/bar']).toBe(overrides['@pnpm.e2e/bar'])
})

test('explicitly specifying a version at install will ignore overrides', async () => {
  prepareEmpty()

  await addDistTag({ package: '@pnpm.e2e/bar', version: '100.0.0', distTag: 'latest' })

  const overrides = {
    '@pnpm.e2e/bar': '100.1.0',
  }
  const EXACT_VERSION = '100.0.0'
  const manifest = await addDependenciesToPackage({},
    [`@pnpm.e2e/bar@${EXACT_VERSION}`],
    await testDefaults({ overrides })
  )

  expect(manifest.dependencies?.['@pnpm.e2e/bar']).toBe(EXACT_VERSION)
})
