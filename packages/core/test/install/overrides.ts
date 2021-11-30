import PnpmError from '@pnpm/error'
import { prepareEmpty } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import { addDependenciesToPackage, mutateModules } from '@pnpm/core'
import {
  testDefaults,
} from '../utils'

test('versions are replaced with versions specified through overrides option', async () => {
  const project = prepareEmpty()

  await addDistTag({ package: 'bar', version: '100.0.0', distTag: 'latest' })
  await addDistTag({ package: 'foo', version: '100.0.0', distTag: 'latest' })

  const overrides = {
    'foobarqar>foo': 'npm:qar@100.0.0',
    'bar@^100.0.0': '100.1.0',
    'dep-of-pkg-with-1-dep': '101.0.0',
  }
  const manifest = await addDependenciesToPackage({},
    ['pkg-with-1-dep@100.0.0', 'foobar@100.0.0', 'foobarqar@1.0.0'],
    await testDefaults({ overrides })
  )

  {
    const lockfile = await project.readLockfile()
    expect(lockfile.packages['/foobarqar/1.0.0'].dependencies?.['foo']).toBe('/qar/100.0.0')
    expect(lockfile.packages['/foobar/100.0.0'].dependencies?.['foo']).toBe('100.0.0')
    expect(lockfile.packages).toHaveProperty(['/dep-of-pkg-with-1-dep/101.0.0'])
    expect(lockfile.packages).toHaveProperty(['/bar/100.1.0'])
    expect(lockfile.overrides).toStrictEqual({
      'foobarqar>foo': 'npm:qar@100.0.0',
      'bar@^100.0.0': '100.1.0',
      'dep-of-pkg-with-1-dep': '101.0.0',
    })
    const currentLockfile = await project.readCurrentLockfile()
    expect(lockfile.overrides).toStrictEqual(currentLockfile.overrides)
  }
  // shall be able to install when package manifest is ignored
  await mutateModules([
    {
      buildIndex: 0,
      manifest,
      mutation: 'install',
      rootDir: process.cwd(),
    },
  ], { ...await testDefaults(), ignorePackageManifest: true, overrides })

  // The lockfile is updated if the overrides are changed
  overrides['bar@^100.0.0'] = '100.0.0'
  // A direct dependency may be overriden as well
  overrides['foobarqar'] = '1.0.1'
  await mutateModules([
    {
      buildIndex: 0,
      manifest,
      mutation: 'install',
      rootDir: process.cwd(),
    },
  ], await testDefaults({ overrides }))

  {
    const lockfile = await project.readLockfile()
    expect(lockfile.packages).toHaveProperty(['/dep-of-pkg-with-1-dep/101.0.0'])
    expect(lockfile.packages).toHaveProperty(['/bar/100.0.0'])
    expect(lockfile.packages).toHaveProperty(['/foobarqar/1.0.1'])
    expect(lockfile.overrides).toStrictEqual({
      foobarqar: '1.0.1',
      'foobarqar>foo': 'npm:qar@100.0.0',
      'bar@^100.0.0': '100.0.0',
      'dep-of-pkg-with-1-dep': '101.0.0',
    })
    const currentLockfile = await project.readCurrentLockfile()
    expect(lockfile.overrides).toStrictEqual(currentLockfile.overrides)
  }

  await mutateModules([
    {
      buildIndex: 0,
      manifest,
      mutation: 'install',
      rootDir: process.cwd(),
    },
  ], await testDefaults({ frozenLockfile: true, overrides }))

  {
    const lockfile = await project.readLockfile()
    expect(lockfile.overrides).toStrictEqual({
      foobarqar: '1.0.1',
      'foobarqar>foo': 'npm:qar@100.0.0',
      'bar@^100.0.0': '100.0.0',
      'dep-of-pkg-with-1-dep': '101.0.0',
    })
    const currentLockfile = await project.readCurrentLockfile()
    expect(lockfile.overrides).toStrictEqual(currentLockfile.overrides)
  }

  overrides['bar@^100.0.0'] = '100.0.1'
  await expect(
    mutateModules([
      {
        buildIndex: 0,
        manifest,
        mutation: 'install',
        rootDir: process.cwd(),
      },
    ], await testDefaults({ frozenLockfile: true, overrides }))
  ).rejects.toThrow(
    new PnpmError('FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE',
      'Cannot perform a frozen installation because the lockfile needs updates'
    )
  )
})
