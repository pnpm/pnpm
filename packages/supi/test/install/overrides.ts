import { prepareEmpty } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import { addDependenciesToPackage, mutateModules } from 'supi'
import {
  testDefaults,
} from '../utils'

test('versions are replaced with versions specified through pnpm.overrides field', async () => {
  const project = prepareEmpty()

  await addDistTag({ package: 'bar', version: '100.0.0', distTag: 'latest' })
  await addDistTag({ package: 'foo', version: '100.0.0', distTag: 'latest' })

  const manifest = await addDependenciesToPackage({
    pnpm: {
      overrides: {
        'foobarqar>foo': 'npm:qar@100.0.0',
        'bar@^100.0.0': '100.1.0',
        'dep-of-pkg-with-1-dep': '101.0.0',
      },
    },
  }, ['pkg-with-1-dep@100.0.0', 'foobar@100.0.0', 'foobarqar@1.0.0'], await testDefaults())

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
  }

  // The lockfile is updated if the overrides are changed
  manifest.pnpm!.overrides!['bar@^100.0.0'] = '100.0.0'
  await mutateModules([
    {
      buildIndex: 0,
      manifest,
      mutation: 'install',
      rootDir: process.cwd(),
    },
  ], await testDefaults())

  {
    const lockfile = await project.readLockfile()
    expect(lockfile.packages).toHaveProperty(['/dep-of-pkg-with-1-dep/101.0.0'])
    expect(lockfile.packages).toHaveProperty(['/bar/100.0.0'])
    expect(lockfile.overrides).toStrictEqual({
      'foobarqar>foo': 'npm:qar@100.0.0',
      'bar@^100.0.0': '100.0.0',
      'dep-of-pkg-with-1-dep': '101.0.0',
    })
  }
})

test('versions are replaced with versions specified through "resolutions" field (for Yarn compatibility)', async () => {
  const project = prepareEmpty()

  await addDistTag({ package: 'bar', version: '100.0.0', distTag: 'latest' })

  const manifest = await addDependenciesToPackage({
    resolutions: {
      'bar@^100.0.0': '100.1.0',
      'dep-of-pkg-with-1-dep': '101.0.0',
    },
  }, ['pkg-with-1-dep@100.0.0', 'foobar@100.0.0'], await testDefaults())

  {
    const lockfile = await project.readLockfile()
    expect(lockfile.packages).toHaveProperty(['/dep-of-pkg-with-1-dep/101.0.0'])
    expect(lockfile.packages).toHaveProperty(['/bar/100.1.0'])
    expect(lockfile.overrides).toStrictEqual({
      'bar@^100.0.0': '100.1.0',
      'dep-of-pkg-with-1-dep': '101.0.0',
    })
  }

  // The lockfile is updated if the resolutions are changed
  manifest.resolutions!['bar@^100.0.0'] = '100.0.0'
  await mutateModules([
    {
      buildIndex: 0,
      manifest,
      mutation: 'install',
      rootDir: process.cwd(),
    },
  ], await testDefaults())

  {
    const lockfile = await project.readLockfile()
    expect(lockfile.packages).toHaveProperty(['/dep-of-pkg-with-1-dep/101.0.0'])
    expect(lockfile.packages).toHaveProperty(['/bar/100.0.0'])
    expect(lockfile.overrides).toStrictEqual({
      'bar@^100.0.0': '100.0.0',
      'dep-of-pkg-with-1-dep': '101.0.0',
    })
  }
})
