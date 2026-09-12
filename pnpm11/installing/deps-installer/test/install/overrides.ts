import fs from 'node:fs'
import path from 'node:path'

import { expect, jest, test } from '@jest/globals'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import { addDependenciesToPackage, type MutatedProject, mutateModules, mutateModulesInSingleProject, type ProjectOptions } from '@pnpm/installing.deps-installer'
import type { LockfileFile } from '@pnpm/lockfile.types'
import { prepare, prepareEmpty, preparePackages } from '@pnpm/prepare'
import type { ResolutionVerifier } from '@pnpm/resolving.resolver-base'
import type { RequestPackageOptions, StoreController } from '@pnpm/store.controller-types'
import { addDistTag } from '@pnpm/testing.registry-mock'
import type { ProjectManifest, ProjectRootDir } from '@pnpm/types'
import { readYamlFileSync } from 'read-yaml-file'

import { testDefaults } from '../utils/index.js'

function trackRequestedPackages (
  storeController: StoreController,
  onRequest?: (requestOptions: RequestPackageOptions) => void
): string[] {
  const requestedPackages: string[] = []
  const requestPackage = storeController.requestPackage
  storeController.requestPackage = async (wantedDependency, requestOptions) => {
    requestedPackages.push(wantedDependency.alias!)
    onRequest?.(requestOptions)
    return requestPackage(wantedDependency, requestOptions)
  }
  return requestedPackages
}

test('adding an exact override reuses the lockfile when the new package has the same dependencies', async () => {
  const project = prepareEmpty()
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/foobarqar': '1.0.0',
    },
  }
  const options = testDefaults()

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  const requestedPackages = trackRequestedPackages(options.storeController)
  options.overrides = {
    '@pnpm.e2e/bar': '100.1.0',
  }

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  expect(requestedPackages).toStrictEqual(['@pnpm.e2e/bar'])
  const lockfile = project.readLockfile()
  expect(lockfile.snapshots['@pnpm.e2e/foobarqar@1.0.0'].dependencies?.['@pnpm.e2e/bar']).toBe('100.1.0')
})

test('an exact override update preserves resolver trust policies', async () => {
  prepareEmpty()
  const reporter = jest.fn()
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/foobarqar': '1.0.0',
    },
  }
  const options = testDefaults({
    handleResolutionPolicyViolations: async () => {},
    hooks: {
      afterAllResolved: [],
      preResolution: [],
      readPackage: [],
    },
    reporter,
    trustPolicy: 'no-downgrade',
    trustPolicyExclude: ['@pnpm.e2e/bar'],
  })

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  reporter.mockClear()
  const verify = jest.fn<ResolutionVerifier['verify']>(async () => ({ ok: true }))
  options.resolutionVerifiers = [{
    canTrustPastCheck: () => false,
    policy: { test: true },
    verify,
  }]
  const requestOptions: RequestPackageOptions[] = []
  const requestedPackages = trackRequestedPackages(
    options.storeController,
    (options) => requestOptions.push(options)
  )
  options.overrides = {
    '@pnpm.e2e/bar': '100.1.0',
  }

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  expect(requestedPackages).toStrictEqual(['@pnpm.e2e/bar'])
  expect(requestOptions[0].trustPolicy).toBe('no-downgrade')
  expect(requestOptions[0].trustPolicyExclude?.('@pnpm.e2e/bar')).toBe(true)
  expect(reporter).not.toHaveBeenCalledWith(expect.objectContaining({
    name: 'pnpm:stage',
    stage: 'resolution_started',
  }))
  expect(verify).toHaveBeenCalled()
  expect(verify.mock.calls).not.toContainEqual([
    expect.anything(),
    expect.objectContaining({ name: '@pnpm.e2e/bar', version: '100.1.0' }),
  ])
})

test('an exact override update reuses the lockfile when the new package has the same dependencies', async () => {
  const project = prepareEmpty()
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/parent-of-pkg-with-1-dep': '1.0.0',
    },
  }
  const options = testDefaults({
    overrides: {
      '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
    },
  })

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  const previousLockfile = project.readLockfile()
  const previousChildResolution = previousLockfile.snapshots['@pnpm.e2e/pkg-with-1-dep@100.0.0']
    .dependencies?.['@pnpm.e2e/dep-of-pkg-with-1-dep']
  expect(previousChildResolution).toBeDefined()
  const requestedPackages = trackRequestedPackages(options.storeController)
  options.overrides = {
    '@pnpm.e2e/pkg-with-1-dep': '100.1.0',
  }

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  expect(requestedPackages).toStrictEqual(['@pnpm.e2e/pkg-with-1-dep'])
  const lockfile = project.readLockfile()
  expect(lockfile.snapshots['@pnpm.e2e/pkg-with-1-dep@100.1.0']
    .dependencies?.['@pnpm.e2e/dep-of-pkg-with-1-dep']).toBe(previousChildResolution)
  expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/pkg-with-1-dep@100.1.0'])
  expect(lockfile.packages).not.toHaveProperty(['@pnpm.e2e/pkg-with-1-dep@100.0.0'])
  const currentLockfile = project.readCurrentLockfile()
  expect(currentLockfile.snapshots['@pnpm.e2e/pkg-with-1-dep@100.1.0']
    .dependencies?.['@pnpm.e2e/dep-of-pkg-with-1-dep']).toBe(previousChildResolution)
})

test('an exact override update falls back to resolution when the package dependencies changed', async () => {
  const project = prepareEmpty()
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/foobarqar': '^1.0.0',
    },
  }
  const options = testDefaults({
    overrides: {
      '@pnpm.e2e/foobarqar': '1.0.0',
    },
  })

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  const requestedPackages = trackRequestedPackages(options.storeController)
  options.overrides = {
    '@pnpm.e2e/foobarqar': '1.0.1',
  }

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  expect(requestedPackages).toContain('@pnpm.e2e/qar')
  const lockfile = project.readLockfile()
  expect(lockfile.snapshots).toHaveProperty(['@pnpm.e2e/foobarqar@1.0.1'])
  expect(lockfile.snapshots['@pnpm.e2e/foobarqar@1.0.1'].dependencies).toHaveProperty(['@pnpm.e2e/qar'])
  expect(lockfile.snapshots['@pnpm.e2e/foobarqar@1.0.1'].dependencies).not.toHaveProperty(['is-positive'])
})

test('a dependency removal override prunes the locked subtree without resolution', async () => {
  const project = prepareEmpty()
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-good-optional': '1.0.0',
    },
  }
  const options = testDefaults()

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  const requestedPackages = trackRequestedPackages(options.storeController)
  options.overrides = {
    'is-positive': '-',
  }

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  expect(requestedPackages).toStrictEqual([])
  const lockfile = project.readLockfile()
  expect(lockfile.snapshots['@pnpm.e2e/pkg-with-good-optional@1.0.0'])
    .not.toHaveProperty(['optionalDependencies', 'is-positive'])
  expect(lockfile.snapshots).not.toHaveProperty(['is-positive@1.0.0'])
  expect(lockfile.packages).not.toHaveProperty(['is-positive@1.0.0'])
  const currentLockfile = project.readCurrentLockfile()
  expect(currentLockfile.snapshots['@pnpm.e2e/pkg-with-good-optional@1.0.0'])
    .not.toHaveProperty(['optionalDependencies', 'is-positive'])
  expect(
    fs.existsSync('node_modules/.pnpm/@pnpm.e2e+pkg-with-good-optional@1.0.0/node_modules/is-positive')
  ).toBe(false)
})

test('a parent-scoped dependency removal override only prunes matching edges', async () => {
  const project = prepareEmpty()
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-good-optional': '1.0.0',
      'is-positive': '1.0.0',
    },
  }
  const options = testDefaults()

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  const requestedPackages = trackRequestedPackages(options.storeController)
  options.overrides = {
    '@pnpm.e2e/pkg-with-good-optional@1>is-positive': '-',
  }

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  expect(requestedPackages).toStrictEqual([])
  const lockfile = project.readLockfile()
  expect(lockfile.importers['.'].dependencies).toHaveProperty(['is-positive'])
  expect(lockfile.snapshots['@pnpm.e2e/pkg-with-good-optional@1.0.0'])
    .not.toHaveProperty(['optionalDependencies', 'is-positive'])
  expect(lockfile.snapshots).toHaveProperty(['is-positive@1.0.0'])
})

test('exact replacements and dependency removals reuse the lockfile together', async () => {
  const project = prepareEmpty()
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/foobarqar': '1.0.0',
    },
  }
  const options = testDefaults()

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  const requestedPackages = trackRequestedPackages(options.storeController)
  options.overrides = {
    '@pnpm.e2e/bar': '100.1.0',
    'is-positive': '-',
  }

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  expect(requestedPackages).toStrictEqual(['@pnpm.e2e/bar'])
  const lockfile = project.readLockfile()
  expect(lockfile.snapshots['@pnpm.e2e/foobarqar@1.0.0'].dependencies?.['@pnpm.e2e/bar']).toBe('100.1.0')
  expect(lockfile.snapshots['@pnpm.e2e/foobarqar@1.0.0'].dependencies).not.toHaveProperty(['is-positive'])
  expect(lockfile.snapshots).not.toHaveProperty(['is-positive@1.0.0'])
})

test('a dependency removal also applies to snapshots rebuilt for replacements', async () => {
  const project = prepareEmpty()
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/parent-of-foobarqar': '1.0.1',
    },
  }
  const options = testDefaults()

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  const requestedPackages = trackRequestedPackages(options.storeController)
  options.overrides = {
    '@pnpm.e2e/foobarqar': '1.0.1',
    '@pnpm.e2e/qar': '-',
  }

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  expect(requestedPackages).toStrictEqual(['@pnpm.e2e/foobarqar'])
  const lockfile = project.readLockfile()
  expect(lockfile.snapshots['@pnpm.e2e/parent-of-foobarqar@1.0.1'].dependencies)
    .not.toHaveProperty(['@pnpm.e2e/qar'])
  expect(lockfile.snapshots['@pnpm.e2e/foobarqar@1.0.1'].dependencies)
    .not.toHaveProperty(['@pnpm.e2e/qar'])
  expect(lockfile.snapshots).not.toHaveProperty(['@pnpm.e2e/qar@100.0.0'])
})

test('an exact override update reuses uniquely compatible locked dependencies and drops obsolete edges', async () => {
  const project = prepareEmpty()
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/parent-of-foobarqar': '1.0.0',
      '@pnpm.e2e/qar': '100.0.0',
    },
  }
  const options = testDefaults({
    overrides: {
      '@pnpm.e2e/foobarqar': '1.0.0',
    },
  })

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  const requestedPackages = trackRequestedPackages(options.storeController)
  options.overrides = {
    '@pnpm.e2e/foobarqar': '1.0.1',
  }

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  expect(requestedPackages).toStrictEqual(['@pnpm.e2e/foobarqar'])
  const lockfile = project.readLockfile()
  expect(lockfile.snapshots['@pnpm.e2e/foobarqar@1.0.1'].dependencies?.['@pnpm.e2e/qar']).toBe('100.0.0')
  expect(lockfile.snapshots['@pnpm.e2e/foobarqar@1.0.1'].dependencies).not.toHaveProperty(['is-positive'])
  expect(lockfile.snapshots).not.toHaveProperty(['is-positive@1.0.0'])
  expect(lockfile.packages).not.toHaveProperty(['is-positive@1.0.0'])
})

test('versions are replaced with versions specified through overrides option', async () => {
  const project = prepareEmpty()

  await addDistTag({ package: '@pnpm.e2e/bar', version: '100.0.0', distTag: 'latest' })
  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.0.0', distTag: 'latest' })

  const overrides: Record<string, string> = {
    '@pnpm.e2e/foobarqar>@pnpm.e2e/foo': 'npm:@pnpm.e2e/qar@100.0.0',
    '@pnpm.e2e/bar@^100.0.0': '100.1.0',
    '@pnpm.e2e/dep-of-pkg-with-1-dep': '101.0.0',
  }
  const { updatedManifest: manifest } = await addDependenciesToPackage({},
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
  const { updatedManifest: manifest } = await addDependenciesToPackage({},
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
  const { updatedManifest: manifest } = await addDependenciesToPackage({},
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

  const lockfile = readYamlFileSync<LockfileFile>(WANTED_LOCKFILE)

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

test('overrides remove dependencies', async () => {
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-good-optional': '1.0.0',
    },
  }

  const project = prepare(manifest)

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({
    overrides: {
      '@pnpm.e2e/pkg-with-good-optional>is-positive': '-',
    },
  }))

  // assert that @pnpm.e2e/pkg-with-good-optional@1.0.0 depends on is-positive@1.0.0
  expect(project.requireModule('@pnpm.e2e/pkg-with-good-optional/package.json')).toMatchObject({
    version: '1.0.0',
    optionalDependencies: {
      'is-positive': '1.0.0',
    },
  })

  // yet because of the overrides, it installs @pnpm.e2e/pkg-with-good-optional@1.0.0 without is-positive@1.0.0
  const lockfile = project.readLockfile()
  expect(lockfile.snapshots).toHaveProperty(['@pnpm.e2e/pkg-with-good-optional@1.0.0'])
  expect(lockfile.snapshots['@pnpm.e2e/pkg-with-good-optional@1.0.0']).not.toHaveProperty(['optionalDependencies', 'is-positive'])
  expect(lockfile.snapshots).not.toHaveProperty(['is-positive@1.0.0'])
  expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/pkg-with-good-optional@1.0.0'])
  expect(lockfile.packages).not.toHaveProperty(['is-positive@1.0.0'])
  expect(
    fs.existsSync('node_modules/.pnpm/@pnpm.e2e+pkg-with-good-optional@1.0.0/node_modules/is-positive')
  ).toBe(false)
  const currentLockfile = project.readCurrentLockfile()
  expect(lockfile.overrides).toStrictEqual(currentLockfile.overrides)
})

// Regression test for https://github.com/pnpm/pnpm/issues/14224
// An override claims the dependency even when it repeats the range the project
// declares, so the declared range is not the update's to move: the overrides
// hook rewrites it back before the resolver reads it, and the lockfile would
// then record a specifier the manifest never shows.
test('update keeps the declared range of a dependency an override repeats', async () => {
  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })
  const project = prepareEmpty()
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/foo': '^100.0.0',
    },
  }
  const options = testDefaults({
    overrides: {
      '@pnpm.e2e/foo': '^100.0.0',
    },
  })

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  const { updatedProject } = await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, { ...options, update: true, updatePackageManifest: true })

  expect(updatedProject.manifest.dependencies).toStrictEqual({ '@pnpm.e2e/foo': '^100.0.0' })
  expect(project.readLockfile().importers['.'].dependencies?.['@pnpm.e2e/foo'].specifier).toBe('^100.0.0')
})

// A range-scoped override claims one declaration of an alias and not another,
// so ownership is decided per declared range rather than per name. Here the
// override matches only the `devDependencies` entry; the `dependencies` one
// the manifest writer reaches is still the update's to move.
test('update moves a declaration a range-scoped override does not claim', async () => {
  await addDistTag({ package: '@pnpm.e2e/foo', version: '100.1.0', distTag: 'latest' })
  const project = prepareEmpty()
  const manifest: ProjectManifest = {
    dependencies: {
      '@pnpm.e2e/foo': '^100.0.0',
    },
    devDependencies: {
      '@pnpm.e2e/foo': '^1.0.0',
    },
  }
  const options = testDefaults({
    overrides: {
      '@pnpm.e2e/foo@^1.0.0': '1.0.0',
    },
  })

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, options)

  const { updatedProject } = await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, { ...options, update: true, updatePackageManifest: true })

  expect(updatedProject.manifest.dependencies).toStrictEqual({ '@pnpm.e2e/foo': '^100.1.0' })
  expect(project.readLockfile().importers['.'].dependencies?.['@pnpm.e2e/foo'].specifier).toBe('^100.1.0')
})
