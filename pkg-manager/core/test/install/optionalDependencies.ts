import fs from 'fs'
import path from 'path'
import { type LockfileV9 as Lockfile } from '@pnpm/lockfile-file'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import { type ProjectRootDir } from '@pnpm/types'
import deepRequireCwd from 'deep-require-cwd'
import { sync as readYamlFile } from 'read-yaml-file'
import {
  addDependenciesToPackage,
  install,
  type MutatedProject,
  mutateModules,
  mutateModulesInSingleProject,
} from '@pnpm/core'
import { sync as rimraf } from '@zkochan/rimraf'
import sinon from 'sinon'
import { testDefaults } from '../utils'

test('successfully install optional dependency with subdependencies', async () => {
  prepareEmpty()

  await addDependenciesToPackage({}, ['fsevents@1.0.14'], testDefaults({ targetDependenciesField: 'optionalDependencies' }))
})

test('skip failing optional dependencies', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-failing-optional-dependency@1.0.0'], testDefaults({ fastUnpack: false }))

  project.has('@pnpm.e2e/pkg-with-failing-optional-dependency/package.json')
})

test('skip failing optional peer dependencies', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-failing-optional-dependency@1.0.0', '@pnpm.e2e/pkg-with-failing-optional-peer@1.0.0'], testDefaults({ fastUnpack: false }))

  const lockfile = project.readLockfile()
  expect(lockfile.snapshots['@pnpm.e2e/pkg-with-failing-optional-peer@1.0.0(@pnpm.e2e/pkg-with-failing-postinstall@1.0.0)'].optionalDependencies).toStrictEqual({
    '@pnpm.e2e/pkg-with-failing-postinstall': '1.0.0',
  })
  expect(lockfile.snapshots['@pnpm.e2e/pkg-with-failing-postinstall@1.0.0'].optional).toBe(true)
})

test('skip non-existing optional dependency', async () => {
  const project = prepareEmpty()

  const reporter = sinon.spy()
  await install({
    dependencies: {
      'is-positive': '1.0.0',
    },
    optionalDependencies: {
      '@pnpm.e2e/i-do-not-exist': '1000',
    },
  }, testDefaults({ reporter }))

  expect(reporter.calledWithMatch({
    package: {
      name: '@pnpm.e2e/i-do-not-exist',
      version: '1000',
    },
    parents: [],
    reason: 'resolution_failure',
  })).toBeTruthy()

  project.has('is-positive')

  const lockfile = project.readLockfile()

  expect(lockfile.importers['.'].dependencies?.['is-positive'].specifier).toBe('1.0.0')
})

test('skip optional dependency that does not support the current OS', async () => {
  const project = prepareEmpty()
  const reporter = sinon.spy()

  let manifest = await install({
    optionalDependencies: {
      '@pnpm.e2e/not-compatible-with-any-os': '*',
    },
  }, testDefaults({ reporter }))

  project.hasNot('@pnpm.e2e/not-compatible-with-any-os')
  project.storeHas('@pnpm.e2e/not-compatible-with-any-os', '1.0.0')
  expect(fs.existsSync(path.resolve('node_modules/.pnpm/@pnpm.e2e+dep-of-optional-pkg@1.0.0'))).toBeFalsy()

  const lockfile = project.readLockfile()
  expect(lockfile.packages['@pnpm.e2e/not-compatible-with-any-os@1.0.0']).toBeTruthy()

  expect(lockfile.packages['@pnpm.e2e/dep-of-optional-pkg@1.0.0']).toBeTruthy()

  const currentLockfile = project.readCurrentLockfile()

  expect(currentLockfile.packages).toStrictEqual(lockfile.packages)

  const modulesInfo = readYamlFile<{ skipped: string[] }>(path.join('node_modules', '.modules.yaml'))
  expect(modulesInfo.skipped).toStrictEqual([
    '@pnpm.e2e/dep-of-optional-pkg@1.0.0',
    '@pnpm.e2e/not-compatible-with-any-os@1.0.0',
  ])

  const logMatcher = sinon.match({
    package: {
      id: '@pnpm.e2e/not-compatible-with-any-os@1.0.0',
      name: '@pnpm.e2e/not-compatible-with-any-os',
      version: '1.0.0',
    },
    reason: 'unsupported_platform',
  })
  const reportedTimes = reporter.withArgs(logMatcher).callCount
  expect(reportedTimes).toBe(1)

  // a previously skipped package is successfully installed

  manifest = await addDependenciesToPackage(manifest, ['@pnpm.e2e/dep-of-optional-pkg'], testDefaults())

  project.has('@pnpm.e2e/dep-of-optional-pkg')

  {
    const modules = project.readModulesManifest()
    expect(modules?.skipped).toStrictEqual(['@pnpm.e2e/not-compatible-with-any-os@1.0.0'])
  }

  rimraf('node_modules')

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ frozenLockfile: true }))

  project.hasNot('@pnpm.e2e/not-compatible-with-any-os')
  project.has('@pnpm.e2e/dep-of-optional-pkg')

  {
    const modules = project.readModulesManifest()
    expect(modules?.skipped).toStrictEqual(['@pnpm.e2e/not-compatible-with-any-os@1.0.0'])
  }
})

test('skip optional dependency that does not support the current Node version', async () => {
  const project = prepareEmpty()
  const reporter = sinon.spy()

  await install({
    optionalDependencies: {
      '@pnpm.e2e/for-legacy-node': '*',
    },
  }, testDefaults({ reporter }))

  project.hasNot('@pnpm.e2e/for-legacy-node')
  project.storeHas('@pnpm.e2e/for-legacy-node', '1.0.0')

  const logMatcher = sinon.match({
    package: {
      id: '@pnpm.e2e/for-legacy-node@1.0.0',
      name: '@pnpm.e2e/for-legacy-node',
      version: '1.0.0',
    },
    reason: 'unsupported_engine',
  })
  const reportedTimes = reporter.withArgs(logMatcher).callCount
  expect(reportedTimes).toBe(1)
})

test('do not skip optional dependency that does not support the current pnpm version', async () => {
  const project = prepareEmpty()
  const reporter = sinon.spy()

  await install({
    optionalDependencies: {
      '@pnpm.e2e/for-legacy-pnpm': '*',
    },
  }, testDefaults({
    reporter,
  }, {}, {}, {
    pnpmVersion: '4.0.0',
  }))

  project.has('@pnpm.e2e/for-legacy-pnpm')
  project.storeHas('@pnpm.e2e/for-legacy-pnpm', '1.0.0')

  const logMatcher = sinon.match({
    package: {
      id: '@pnpm.e2e/for-legacy-pnpm@1.0.0',
      name: '@pnpm.e2e/for-legacy-pnpm',
      version: '1.0.0',
    },
    reason: 'unsupported_engine',
  })
  const reportedTimes = reporter.withArgs(logMatcher).callCount
  expect(reportedTimes).toBe(0)
})

test('don\'t skip optional dependency that does not support the current OS when forcing', async () => {
  const project = prepareEmpty()

  await install({
    optionalDependencies: {
      '@pnpm.e2e/not-compatible-with-any-os': '*',
    },
  }, testDefaults({}, {}, {}, { force: true }))

  project.has('@pnpm.e2e/not-compatible-with-any-os')
  project.storeHas('@pnpm.e2e/not-compatible-with-any-os', '1.0.0')
})

// Covers https://github.com/pnpm/pnpm/issues/2636
test('optional subdependency is not removed from current lockfile when new dependency added', async () => {
  preparePackages([
    {
      location: 'project-1',
      package: { name: 'project-1' },
    },
    {
      location: 'project-2',
      package: { name: 'project-2' },
    },
  ])

  const importers: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
  ]
  const allProjects = [
    {
      buildIndex: 0,
      manifest: {
        name: 'project-1',
        version: '1.0.0',

        dependencies: {
          '@pnpm.e2e/pkg-with-optional': '1.0.0',
        },
      },
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'project-2',
        version: '1.0.0',
      },
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
  ]
  await mutateModules(importers,
    testDefaults({ allProjects, hoistPattern: ['*'] })
  )

  {
    const modulesInfo = readYamlFile<{ skipped: string[] }>(path.join('node_modules', '.modules.yaml'))
    expect(modulesInfo.skipped).toStrictEqual(['@pnpm.e2e/dep-of-optional-pkg@1.0.0', '@pnpm.e2e/not-compatible-with-any-os@1.0.0'])

    const currentLockfile = readYamlFile<Lockfile>(path.resolve('node_modules/.pnpm/lock.yaml'))
    expect(currentLockfile.packages).toHaveProperty(['@pnpm.e2e/not-compatible-with-any-os@1.0.0'])
  }

  await mutateModules([
    {
      ...importers[0],
      dependencySelectors: ['is-positive@1.0.0'],
      mutation: 'installSome',
    },
  ], testDefaults({ allProjects, fastUnpack: false, hoistPattern: ['*'] }))

  {
    const currentLockfile = readYamlFile<Lockfile>(path.resolve('node_modules/.pnpm/lock.yaml'))
    expect(currentLockfile.packages).toHaveProperty(['@pnpm.e2e/not-compatible-with-any-os@1.0.0'])
  }
})

test('optional subdependency is skipped', async () => {
  const project = prepareEmpty()
  const reporter = sinon.spy()

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-optional', '@pnpm.e2e/dep-of-optional-pkg'], testDefaults({ reporter }))

  {
    const modulesInfo = readYamlFile<{ skipped: string[] }>(path.join('node_modules', '.modules.yaml'))
    expect(modulesInfo.skipped).toStrictEqual(['@pnpm.e2e/not-compatible-with-any-os@1.0.0'])
  }

  expect(fs.existsSync('node_modules/.pnpm/@pnpm.e2e+pkg-with-optional@1.0.0')).toBeTruthy()
  expect(fs.existsSync('node_modules/.pnpm/@pnpm.e2e+not-compatible-with-any-os@1.0.0')).toBeFalsy()

  const logMatcher = sinon.match({
    package: {
      id: '@pnpm.e2e/not-compatible-with-any-os@1.0.0',
      name: '@pnpm.e2e/not-compatible-with-any-os',
      version: '1.0.0',
    },
    reason: 'unsupported_platform',
  })
  const reportedTimes = reporter.withArgs(logMatcher).callCount
  expect(reportedTimes).toBe(1)

  // recreate the lockfile with optional dependencies present

  expect(fs.existsSync('pnpm-lock.yaml')).toBeTruthy()
  rimraf('pnpm-lock.yaml')

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults()
  )

  const lockfile = project.readLockfile()

  expect(Object.keys(lockfile.packages).length).toBe(3)
  expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/not-compatible-with-any-os@1.0.0'])

  // forced headless install should install non-compatible optional deps

  // TODO: move next case to @pnpm/headless tests
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ force: true, frozenLockfile: true }))

  expect(fs.existsSync('node_modules/.pnpm/@pnpm.e2e+not-compatible-with-any-os@1.0.0')).toBeTruthy()

  {
    const modulesInfo = readYamlFile<{ skipped: string[] }>(path.join('node_modules', '.modules.yaml'))
    expect(modulesInfo.skipped).toStrictEqual([])
  }
})

// Covers https://github.com/pnpm/pnpm/issues/2663
test('optional subdependency of newly added optional dependency is skipped', async () => {
  const project = prepareEmpty()
  const reporter = sinon.spy()

  await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-optional'], testDefaults({ reporter, targetDependenciesField: 'optionalDependencies' }))

  const modulesInfo = readYamlFile<{ skipped: string[] }>(path.join('node_modules', '.modules.yaml'))
  expect(modulesInfo.skipped).toStrictEqual(['@pnpm.e2e/dep-of-optional-pkg@1.0.0', '@pnpm.e2e/not-compatible-with-any-os@1.0.0'])

  const lockfile = project.readLockfile()

  expect(Object.keys(lockfile.packages).length).toBe(3)
  expect(lockfile.packages).toHaveProperty(['@pnpm.e2e/not-compatible-with-any-os@1.0.0'])
})

test('only that package is skipped which is an optional dependency only and not installable', async () => {
  const project = prepareEmpty()
  const reporter = sinon.spy()

  const manifest = await addDependenciesToPackage({}, [
    '@pnpm.e2e/peer-c@1.0.0',
    '@pnpm.e2e/has-optional-dep-with-peer',
    '@pnpm.e2e/not-compatible-with-any-os-and-has-peer',
  ], testDefaults({ reporter }))

  {
    const modulesInfo = readYamlFile<{ skipped: string[] }>(path.join('node_modules', '.modules.yaml'))
    expect(modulesInfo.skipped).toStrictEqual([])
  }

  const lockfile = project.readLockfile()
  expect(typeof lockfile.snapshots['@pnpm.e2e/dep-of-optional-pkg@1.0.0'].optional).toBe('undefined')

  rimraf('node_modules')

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults({ frozenLockfile: true }))

  {
    const modulesInfo = readYamlFile<{ skipped: string[] }>(path.join('node_modules', '.modules.yaml'))
    expect(modulesInfo.skipped).toStrictEqual([])
  }
})

test('not installing optional dependencies when optional is false', async () => {
  const project = prepareEmpty()

  await install(
    {
      dependencies: {
        '@pnpm.e2e/pkg-with-good-optional': '*',
      },
      optionalDependencies: {
        'is-positive': '1.0.0',
      },
    },
    testDefaults({
      include: {
        dependencies: true,
        devDependencies: true,
        optionalDependencies: false,
      },
    })
  )

  project.hasNot('is-positive')
  project.has('@pnpm.e2e/pkg-with-good-optional')

  expect(deepRequireCwd(['@pnpm.e2e/pkg-with-good-optional', '@pnpm.e2e/dep-of-pkg-with-1-dep', './package.json'])).toBeTruthy()
  expect(deepRequireCwd.silent(['@pnpm.e2e/pkg-with-good-optional', 'is-positive', './package.json'])).toBeFalsy()
})

test('optional dependency has bigger priority than regular dependency', async () => {
  prepareEmpty()

  await install({
    dependencies: {
      'is-positive': '1.0.0',
    },
    optionalDependencies: {
      'is-positive': '3.1.0',
    },
  }, testDefaults())

  expect(deepRequireCwd(['is-positive', './package.json']).version).toBe('3.1.0')
})

// Covers https://github.com/pnpm/pnpm/issues/1386
// TODO: use smaller packages to cover the test case
test('only skip optional dependencies', async () => {
  /*
    @google-cloud/functions-emulator has various dependencies, one of them is duplexify.
    duplexify depends on stream-shift. As duplexify is a dependency of an optional dependency
    and @google-cloud/functions-emulator won't be installed, duplexify and stream-shift
    are marked as skipped.
    firebase-tools also depends on duplexify and stream-shift, through got@3.3.1.
    Make sure that duplexify and stream-shift are installed because they are needed
    by firebase-tools, even if they were marked as skipped earlier.
  */

  prepareEmpty()

  const preferVersion = (selector: string) => ({ [selector]: 'version' as const })
  const preferredVersions = {
    duplexify: preferVersion('3.6.0'),
    got: preferVersion('3.3.1'),
    'stream-shift': preferVersion('1.0.0'),
  }
  await install({
    dependencies: {
      'firebase-tools': '4.2.1',
    },
    optionalDependencies: {
      '@google-cloud/functions-emulator': '1.0.0-beta.5',
    },
  }, testDefaults({ fastUnpack: false, preferredVersions }))

  expect(fs.existsSync(path.resolve('node_modules/.pnpm/duplexify@3.6.0'))).toBeTruthy()
  expect(fs.existsSync(path.resolve('node_modules/.pnpm/stream-shift@1.0.0'))).toBeTruthy()

  expect(fs.existsSync(path.resolve('node_modules/.pnpm/got@3.3.1/node_modules/duplexify'))).toBeTruthy()
})

test('skip optional dependency that does not support the current OS, when doing install on a subset of workspace projects', async () => {
  preparePackages([
    {
      name: 'project1',
    },
    {
      name: 'project2',
    },
  ])

  const [{ manifest }] = (await mutateModules(
    [
      {
        mutation: 'install',
        rootDir: path.resolve('project1') as ProjectRootDir,
      },
      {
        mutation: 'install',
        rootDir: path.resolve('project2') as ProjectRootDir,
      },
    ],
    testDefaults({
      allProjects: [
        {
          buildIndex: 0,
          manifest: {
            name: 'project1',
            version: '1.0.0',

            optionalDependencies: {
              '@pnpm.e2e/not-compatible-with-any-os': '*',
            },
          },
          rootDir: path.resolve('project1') as ProjectRootDir,
        },
        {
          buildIndex: 0,
          manifest: {
            name: 'project2',
            version: '1.0.0',

            dependencies: {
              '@pnpm.e2e/pkg-with-1-dep': '100.0.0',
            },
          },
          rootDir: path.resolve('project2') as ProjectRootDir,
        },
      ],
      lockfileDir: process.cwd(),
      lockfileOnly: true,
    })
  )).updatedProjects

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: path.resolve('project1') as ProjectRootDir,
  }, testDefaults({
    frozenLockfile: false,
    lockfileDir: process.cwd(),
    preferFrozenLockfile: false,
  }))

  const modulesInfo = readYamlFile<{ skipped: string[] }>(path.join('node_modules', '.modules.yaml'))
  expect(modulesInfo.skipped).toStrictEqual([
    '@pnpm.e2e/dep-of-optional-pkg@1.0.0',
    '@pnpm.e2e/not-compatible-with-any-os@1.0.0',
  ])
})

test('do not fail on unsupported dependency of optional dependency', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['@pnpm.e2e/not-compatible-with-not-compatible-dep@1.0.0'],
    testDefaults({ targetDependenciesField: 'optionalDependencies' }, {}, {}, { engineStrict: true })
  )

  const lockfile = project.readLockfile()
  expect(lockfile.snapshots['@pnpm.e2e/not-compatible-with-any-os@1.0.0'].optional).toBeTruthy()
  expect(lockfile.snapshots['@pnpm.e2e/dep-of-optional-pkg@1.0.0']).toBeTruthy()
})

test('fail on unsupported dependency of optional dependency', async () => {
  prepareEmpty()
  await expect(
    addDependenciesToPackage(
      {},
      ['@pnpm.e2e/has-not-compatible-dep@1.0.0'],
      testDefaults({ targetDependenciesField: 'optionalDependencies' }, {}, {}, { engineStrict: true })
    )
  ).rejects.toThrow()
})

test('do not fail on an optional dependency that has a non-optional dependency with a failing postinstall script', async () => {
  prepareEmpty()
  await expect(
    addDependenciesToPackage(
      {},
      ['@pnpm.e2e/has-failing-postinstall-dep@1.0.0'],
      testDefaults({ targetDependenciesField: 'optionalDependencies' })
    )
  ).resolves.toBeTruthy()
})

test('fail on a package with failing postinstall if the package is both an optional and non-optional dependency', async () => {
  prepareEmpty()
  await expect(
    install(
      {
        dependencies: {
          '@pnpm.e2e/failing-postinstall': '1.0.0',
        },
        optionalDependencies: {
          '@pnpm.e2e/has-failing-postinstall-dep': '1.0.0',
        },
      },
      testDefaults({})
    )
  ).rejects.toThrow()
})

describe('supported architectures', () => {
  test.each(['isolated', 'hoisted'])('install optional dependency for the supported architecture set by the user (nodeLinker=%s)', async (nodeLinker) => {
    prepareEmpty()
    const opts = testDefaults({ nodeLinker })

    const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/has-many-optional-deps@1.0.0'], {
      ...opts,
      supportedArchitectures: { os: ['darwin'], cpu: ['arm64'] },
    })
    expect(deepRequireCwd(['@pnpm.e2e/has-many-optional-deps', '@pnpm.e2e/darwin-arm64', './package.json']).version).toBe('1.0.0')

    await install(manifest, {
      ...opts,
      preferFrozenLockfile: false,
      supportedArchitectures: { os: ['darwin'], cpu: ['x64'] },
    })
    expect(deepRequireCwd(['@pnpm.e2e/has-many-optional-deps', '@pnpm.e2e/darwin-x64', './package.json']).version).toBe('1.0.0')

    await install(manifest, {
      ...opts,
      frozenLockfile: true,
      supportedArchitectures: { os: ['linux'], cpu: ['x64'] },
    })
    expect(deepRequireCwd(['@pnpm.e2e/has-many-optional-deps', '@pnpm.e2e/linux-x64', './package.json']).version).toBe('1.0.0')
  })
  test('remove optional dependencies that are not used', async () => {
    prepareEmpty()
    const opts = testDefaults({ modulesCacheMaxAge: 0 })

    const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/has-many-optional-deps@1.0.0'], {
      ...opts,
      supportedArchitectures: { os: ['darwin', 'linux', 'win32'], cpu: ['arm64', 'x64'] },
    })

    await install(manifest, {
      ...opts,
      supportedArchitectures: { os: ['darwin'], cpu: ['x64'] },
    })
    expect(fs.readdirSync('node_modules/.pnpm').length).toBe(3)
  })
  test('remove optional dependencies that are not used, when hoisted node linker is used', async () => {
    prepareEmpty()
    const opts = testDefaults({ nodeLinker: 'hoisted' })

    const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/has-many-optional-deps@1.0.0'], {
      ...opts,
      supportedArchitectures: { os: ['darwin', 'linux', 'win32'], cpu: ['arm64', 'x64'] },
    })

    await install(manifest, {
      ...opts,
      supportedArchitectures: { os: ['darwin'], cpu: ['x64'] },
    })
    expect(fs.readdirSync('node_modules/@pnpm.e2e').sort()).toStrictEqual(['darwin-x64', 'has-many-optional-deps'])
  })
  test('remove optional dependencies if supported architectures have changed and a new dependency is added', async () => {
    prepareEmpty()
    const opts = testDefaults({ modulesCacheMaxAge: 0 })

    const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/parent-of-has-many-optional-deps@1.0.0'], {
      ...opts,
      supportedArchitectures: { os: ['darwin', 'linux', 'win32'], cpu: ['arm64', 'x64'] },
    })

    await addDependenciesToPackage(manifest, ['is-positive@1.0.0'], {
      ...opts,
      supportedArchitectures: { os: ['darwin'], cpu: ['x64'] },
    })
    expect(fs.readdirSync('node_modules/.pnpm').length).toBe(5)
  })
})

test('optional dependency is hardlinked to the store if it does not require a build', async () => {
  prepareEmpty()
  const manifest = {
    dependencies: {
      '@pnpm.e2e/pkg-with-good-optional': '*',
    },
  }

  const reporter = jest.fn()
  await install(manifest, testDefaults({ reporter }, {}, {}, { packageImportMethod: 'hardlink' }))

  expect(reporter).toHaveBeenCalledWith(
    expect.objectContaining({
      level: 'debug',
      name: 'pnpm:progress',
      method: 'hardlink',
      status: 'imported',
      to: path.resolve('node_modules/.pnpm/is-positive@1.0.0/node_modules/is-positive'),
    })
  )

  rimraf('node_modules')

  reporter.mockClear()
  await install(manifest, testDefaults({ frozenLockfile: true, reporter }, {}, {}, { packageImportMethod: 'hardlink' }))

  expect(reporter).toHaveBeenCalledWith(
    expect.objectContaining({
      level: 'debug',
      name: 'pnpm:progress',
      method: 'hardlink',
      status: 'imported',
      to: path.resolve('node_modules/.pnpm/is-positive@1.0.0/node_modules/is-positive'),
    })
  )
})

// Covers https://github.com/pnpm/pnpm/issues/7943
test('complex scenario with same optional dependencies appearing in many places of the dependency graph', async () => {
  prepareEmpty()
  await addDependenciesToPackage({}, ['@storybook/addon-essentials@7.6.17', 'storybook@7.6.17', 'vite@5.2.8'], testDefaults())

  expect(fs.readdirSync('node_modules/.pnpm/esbuild@0.18.20/node_modules/@esbuild').length).toEqual(1)
  expect(fs.readdirSync('node_modules/.pnpm/esbuild@0.20.2/node_modules/@esbuild').length).toEqual(1)
})

// Covers https://github.com/pnpm/pnpm/issues/8066
test('dependency that is both optional and non-optional is installed, when optional dependencies should be skipped', async () => {
  prepareEmpty()
  await addDependenciesToPackage({}, ['@babel/cli@7.24.5', 'del@6.1.1'], testDefaults({
    include: {
      dependencies: true,
      optionalDependencies: false,
      devDependencies: true,
    },
  }))

  const dirs = fs.readdirSync('node_modules/.pnpm')
  expect(dirs.find(dir => dir.startsWith('fill-range@'))).toBeDefined()
})
