import path from 'path'
import fs from 'fs'
import { sync as readYamlFile } from 'read-yaml-file'
import { PnpmError } from '@pnpm/error'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { type MutatedProject, type ProjectOptions, addDependenciesToPackage, mutateModulesInSingleProject, mutateModules } from '@pnpm/core'
import { type LockfileFileV9 } from '@pnpm/lockfile-types'
import { type ProjectRootDir, type ProjectManifest } from '@pnpm/types'
import {
  testDefaults,
} from '../utils'

test('versions are replaced with versions specified through overrides option', async () => {
  const project = prepareEmpty()

  await addDistTag({ package: '@pnpm.e2e/bar', version: '100.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.0.0', distTag: 'latest' })

  const overrides: Record<string, string> = {
    '@pnpm.e2e/foobarqar>@pnpm.e2e/foo': 'npm:@pnpm.e2e/qar@100.0.0',
    '@pnpm.e2e/bar@^100.0.0': '100.1.0',
    '@pnpm.e2e/dep-of-pkg-with-1-dep': '101.0.0',
  }
  const manifest = await addDependenciesToPackage({},
    ['@pnpm.e2e/pkg-with-1-dep@100.0.0', '@pnpm.e2e/foobar@100.0.0', '@pnpm.e2e/foobarqar@1.0.0'],
    testDefaults({ overrides })
  )

  {
    const lockfile = project.readLockfile()
    expect(lockfile.snapshots['@pnpm.e2e/foobarqar@1.0.0'].dependencies?.['@pnpm.e2e/foo']).toBe('@pnpm.e2e/qar@100.0.0')
    expect(lockfile.snapshots['@pnpm.e2e/foobar@100.0.0'].dependencies?.['@pnpm.e2e/foo']).toBe('100.0.0')
    expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@101.0.0'])
    expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/bar@100.1.0'])
    expect(lockfile.overrides).toStrictEqual({
      '@pnpm.e2e/foobarqar>@pnpm.e2e/foo': 'npm:@pnpm.e2e/qar@100.0.0',
      '@pnpm.e2e/bar@^100.0.0': '100.1.0',
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '101.0.0',
    })
    const currentLockfile = project.readCurrentLockfile()
    expect(lockfile.overrides).toStrictEqual(currentLockfile.overrides)
  }
  // shall be able to install when package manifest is ignored
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, { ...testDefaults(), ignorePackageManifest: true, overrides })

  // The lockfile is updated if the overrides are changed
  overrides['@pnpm.e2e/bar@^100.0.0'] = '100.0.0'
  // A direct dependency may be overridden as well
  overrides['@pnpm.e2e/foobarqar'] = '1.0.1'
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ overrides }))

  {
    const lockfile = project.readLockfile()
    expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/dep-of-pkg-with-1-dep@101.0.0'])
    expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/bar@100.0.0'])
    expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/foobarqar@1.0.1'])
    expect(lockfile.overrides).toStrictEqual({
      '@pnpm.e2e/foobarqar': '1.0.1',
      '@pnpm.e2e/foobarqar>@pnpm.e2e/foo': 'npm:@pnpm.e2e/qar@100.0.0',
      '@pnpm.e2e/bar@^100.0.0': '100.0.0',
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '101.0.0',
    })
    const currentLockfile = project.readCurrentLockfile()
    expect(lockfile.overrides).toStrictEqual(currentLockfile.overrides)
  }

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ frozenLockfile: true, overrides }))

  {
    const lockfile = project.readLockfile()
    expect(lockfile.overrides).toStrictEqual({
      '@pnpm.e2e/foobarqar': '1.0.1',
      '@pnpm.e2e/foobarqar>@pnpm.e2e/foo': 'npm:@pnpm.e2e/qar@100.0.0',
      '@pnpm.e2e/bar@^100.0.0': '100.0.0',
      '@pnpm.e2e/dep-of-pkg-with-1-dep': '101.0.0',
    })
    const currentLockfile = project.readCurrentLockfile()
    expect(lockfile.overrides).toStrictEqual(currentLockfile.overrides)
  }

  overrides['@pnpm.e2e/bar@^100.0.0'] = '100.0.1'
  await expect(
    mutateModulesInSingleProject({
      manifest,
      mutation: 'install',
      rootDir: process.cwd() as ProjectRootDir,
    }, testDefaults({ frozenLockfile: true, overrides }))
  ).rejects.toThrow(
    new PnpmError('LOCKFILE_CONFIG_MISMATCH',
      'Cannot proceed with the frozen installation. The current "overrides" configuration doesn\'t match the value found in the lockfile'
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
    testDefaults({ overrides })
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
    testDefaults({ overrides })
  )

  expect(manifest.dependencies?.['@pnpm.e2e/bar']).toBe(EXACT_VERSION)
})

test('overrides with local file and link specs', async () => {
  interface LocationAndManifest {
    location: string
    package: ProjectManifest
  }
  const root: LocationAndManifest = {
    location: '.',
    package: {
      name: 'root',
    },
  }
  const direct: LocationAndManifest = {
    location: 'packages/direct',
    package: {
      name: 'direct',
      dependencies: {
        'relative-file-pkg': '*',
        'absolute-file-pkg': '*',
        'relative-link-pkg': '*',
        'absolute-link-pkg': '*',
      },
    },
  }
  const indirect: LocationAndManifest = {
    location: 'packages/indirect',
    package: {
      name: 'indirect',
      dependencies: {
        '@pnpm.e2e/depends-on-pkg-abcd': '1.0.0',
      },
    },
  }
  const pkg: LocationAndManifest = {
    location: 'overrides/pkg',
    package: {
      name: 'pkg',
      version: '0.0.0',
    },
  }
  preparePackages([
    root,
    direct,
    indirect,
    pkg,
  ])

  const importers = [root, direct, indirect].map(({ location }): MutatedProject => ({
    mutation: 'install',
    rootDir: path.resolve(location) as ProjectRootDir,
  }))
  const allProjects = [root, direct, indirect].map((input): ProjectOptions => ({
    buildIndex: 0,
    manifest: input.package,
    rootDir: path.resolve(input.location) as ProjectRootDir,
  }))
  await mutateModules(importers, {
    ...testDefaults({ allProjects }),
    overrides: {
      'relative-file-pkg': 'file:./overrides/pkg',
      'absolute-file-pkg': `file:${path.resolve('overrides/pkg')}`,
      'relative-link-pkg': 'link:./overrides/pkg',
      'absolute-link-pkg': `link:${path.resolve('overrides/pkg')}`,
      '@pnpm.e2e/pkg-a': 'file:./overrides/pkg',
      '@pnpm.e2e/pkg-b': `file:${path.resolve('overrides/pkg')}`,
      '@pnpm.e2e/pkg-c': 'link:./overrides/pkg',
      '@pnpm.e2e/pkg-d': `link:${path.resolve('overrides/pkg')}`,
    },
  })

  const lockfile = readYamlFile<LockfileFileV9>(WANTED_LOCKFILE)

  expect(lockfile.importers?.['packages/direct']).toStrictEqual({
    dependencies: {
      'relative-file-pkg': {
        specifier: 'file:../../overrides/pkg',
        version: 'pkg@file:overrides/pkg',
      },
      'absolute-file-pkg': {
        specifier: `file:${path.resolve('overrides/pkg')}`,
        version: 'pkg@file:overrides/pkg',
      },
      'relative-link-pkg': {
        specifier: 'link:../../overrides/pkg',
        version: 'link:../../overrides/pkg',
      },
      'absolute-link-pkg': {
        specifier: `link:${path.resolve('overrides/pkg')}`,
        version: 'link:../../overrides/pkg',
      },
    },
  })

  expect(lockfile.snapshots?.['@pnpm.e2e/depends-on-pkg-abcd@1.0.0']).toStrictEqual({
    dependencies: {
      '@pnpm.e2e/pkg-a': 'pkg@file:overrides/pkg',
      '@pnpm.e2e/pkg-b': 'pkg@file:overrides/pkg',
      '@pnpm.e2e/pkg-c': 'link:overrides/pkg',
      '@pnpm.e2e/pkg-d': 'link:overrides/pkg',
    },
  })

  const directPrefix = 'packages/direct/node_modules'
  expect(fs.realpathSync(path.join(directPrefix, 'absolute-file-pkg'))).toBe(path.resolve('node_modules/.pnpm/pkg@file+overrides+pkg/node_modules/pkg'))
  expect(fs.realpathSync(path.join(directPrefix, 'relative-file-pkg'))).toBe(path.resolve('node_modules/.pnpm/pkg@file+overrides+pkg/node_modules/pkg'))
  expect(fs.realpathSync(path.join(directPrefix, 'absolute-link-pkg'))).toBe(path.resolve('overrides/pkg'))
  expect(fs.realpathSync(path.join(directPrefix, 'relative-link-pkg'))).toBe(path.resolve('overrides/pkg'))

  const indirectPrefix = 'node_modules/.pnpm/@pnpm.e2e+depends-on-pkg-abcd@1.0.0/node_modules'
  expect(fs.realpathSync(path.join(indirectPrefix, '@pnpm.e2e/pkg-a'))).toBe(path.resolve('node_modules/.pnpm/pkg@file+overrides+pkg/node_modules/pkg'))
  expect(fs.realpathSync(path.join(indirectPrefix, '@pnpm.e2e/pkg-b'))).toBe(path.resolve('node_modules/.pnpm/pkg@file+overrides+pkg/node_modules/pkg'))
  expect(fs.realpathSync(path.join(indirectPrefix, '@pnpm.e2e/pkg-c'))).toBe(path.resolve('overrides/pkg'))
  expect(fs.realpathSync(path.join(indirectPrefix, '@pnpm.e2e/pkg-d'))).toBe(path.resolve('overrides/pkg'))
})
