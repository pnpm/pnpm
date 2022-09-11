import path from 'path'
import { Lockfile } from '@pnpm/lockfile-file'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import readYamlFile from 'read-yaml-file'
import {
  addDependenciesToPackage,
  install,
  MutatedProject,
  mutateModules,
  mutateModulesInSingleProject,
} from '@pnpm/core'
import rimraf from '@zkochan/rimraf'
import exists from 'path-exists'
import sinon from 'sinon'
import deepRequireCwd from 'deep-require-cwd'
import { testDefaults } from '../utils'

test('successfully install optional dependency with subdependencies', async () => {
  prepareEmpty()

  await addDependenciesToPackage({}, ['fsevents@1.0.14'], await testDefaults({ targetDependenciesField: 'optionalDependencies' }))
})

test('skip failing optional dependencies', async () => {
  const project = prepareEmpty()
  await addDependenciesToPackage({}, ['pkg-with-failing-optional-dependency@1.0.1'], await testDefaults({ fastUnpack: false }))

  const m = project.requireModule('pkg-with-failing-optional-dependency')
  expect(m(-1)).toBeTruthy()
})

test('skip non-existing optional dependency', async () => {
  const project = prepareEmpty()

  const reporter = sinon.spy()
  await install({
    dependencies: {
      'is-positive': '*',
    },
    optionalDependencies: {
      '@pnpm.e2e/i-do-not-exist': '1000',
    },
  }, await testDefaults({ reporter }))

  expect(reporter.calledWithMatch({
    package: {
      name: '@pnpm.e2e/i-do-not-exist',
      version: '1000',
    },
    parents: [],
    reason: 'resolution_failure',
  })).toBeTruthy()

  await project.has('is-positive')

  const lockfile = await project.readLockfile()

  expect(lockfile.specifiers).toStrictEqual({ 'is-positive': '*' })
})

test('skip optional dependency that does not support the current OS', async () => {
  const project = prepareEmpty()
  const reporter = sinon.spy()

  let manifest = await install({
    optionalDependencies: {
      '@pnpm.e2e/not-compatible-with-any-os': '*',
    },
  }, await testDefaults({ reporter }))

  await project.hasNot('@pnpm.e2e/not-compatible-with-any-os')
  await project.storeHas('@pnpm.e2e/not-compatible-with-any-os', '1.0.0')
  expect(await exists(path.resolve('node_modules/.pnpm/@pnpm.e2e+dep-of-optional-pkg@1.0.0'))).toBeFalsy()

  const lockfile = await project.readLockfile()
  expect(lockfile.packages['/@pnpm.e2e/not-compatible-with-any-os/1.0.0']).toBeTruthy()

  // optional dependencies always get requiresBuild: true
  // this is to resolve https://github.com/pnpm/pnpm/issues/2038
  expect(lockfile.packages['/@pnpm.e2e/not-compatible-with-any-os/1.0.0'].requiresBuild).toBeTruthy()

  expect(lockfile.packages['/@pnpm.e2e/dep-of-optional-pkg/1.0.0']).toBeTruthy()

  const currentLockfile = await project.readCurrentLockfile()

  expect(currentLockfile.packages).toStrictEqual(lockfile.packages)

  const modulesInfo = await readYamlFile<{ skipped: string[] }>(path.join('node_modules', '.modules.yaml'))
  expect(modulesInfo.skipped).toStrictEqual([
    '/@pnpm.e2e/dep-of-optional-pkg/1.0.0',
    '/@pnpm.e2e/not-compatible-with-any-os/1.0.0',
  ])

  const logMatcher = sinon.match({
    package: {
      id: `localhost+${REGISTRY_MOCK_PORT}/@pnpm.e2e/not-compatible-with-any-os/1.0.0`,
      name: '@pnpm.e2e/not-compatible-with-any-os',
      version: '1.0.0',
    },
    reason: 'unsupported_platform',
  })
  const reportedTimes = reporter.withArgs(logMatcher).callCount
  expect(reportedTimes).toBe(1)

  // a previously skipped package is successfully installed

  manifest = await addDependenciesToPackage(manifest, ['@pnpm.e2e/dep-of-optional-pkg'], await testDefaults())

  await project.has('@pnpm.e2e/dep-of-optional-pkg')

  {
    const modules = await project.readModulesManifest()
    expect(modules?.skipped).toStrictEqual(['/@pnpm.e2e/not-compatible-with-any-os/1.0.0'])
  }

  await rimraf('node_modules')

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd(),
  }, await testDefaults({ frozenLockfile: true }))

  await project.hasNot('@pnpm.e2e/not-compatible-with-any-os')
  await project.has('@pnpm.e2e/dep-of-optional-pkg')

  {
    const modules = await project.readModulesManifest()
    expect(modules?.skipped).toStrictEqual(['/@pnpm.e2e/not-compatible-with-any-os/1.0.0'])
  }
})

test('skip optional dependency that does not support the current Node version', async () => {
  const project = prepareEmpty()
  const reporter = sinon.spy()

  await install({
    optionalDependencies: {
      '@pnpm.e2e/for-legacy-node': '*',
    },
  }, await testDefaults({ reporter }))

  await project.hasNot('@pnpm.e2e/for-legacy-node')
  await project.storeHas('@pnpm.e2e/for-legacy-node', '1.0.0')

  const logMatcher = sinon.match({
    package: {
      id: `localhost+${REGISTRY_MOCK_PORT}/@pnpm.e2e/for-legacy-node/1.0.0`,
      name: '@pnpm.e2e/for-legacy-node',
      version: '1.0.0',
    },
    reason: 'unsupported_engine',
  })
  const reportedTimes = reporter.withArgs(logMatcher).callCount
  expect(reportedTimes).toBe(1)
})

test('skip optional dependency that does not support the current pnpm version', async () => {
  const project = prepareEmpty()
  const reporter = sinon.spy()

  await install({
    optionalDependencies: {
      '@pnpm.e2e/for-legacy-pnpm': '*',
    },
  }, await testDefaults({
    reporter,
  }, {}, {}, {
    pnpmVersion: '4.0.0',
  }))

  await project.hasNot('@pnpm.e2e/for-legacy-pnpm')
  await project.storeHas('@pnpm.e2e/for-legacy-pnpm', '1.0.0')

  const logMatcher = sinon.match({
    package: {
      id: `localhost+${REGISTRY_MOCK_PORT}/@pnpm.e2e/for-legacy-pnpm/1.0.0`,
      name: '@pnpm.e2e/for-legacy-pnpm',
      version: '1.0.0',
    },
    reason: 'unsupported_engine',
  })
  const reportedTimes = reporter.withArgs(logMatcher).callCount
  expect(reportedTimes).toBe(1)
})

test('don\'t skip optional dependency that does not support the current OS when forcing', async () => {
  const project = prepareEmpty()

  await install({
    optionalDependencies: {
      '@pnpm.e2e/not-compatible-with-any-os': '*',
    },
  }, await testDefaults({}, {}, {}, { force: true }))

  await project.has('@pnpm.e2e/not-compatible-with-any-os')
  await project.storeHas('@pnpm.e2e/not-compatible-with-any-os', '1.0.0')
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
      rootDir: path.resolve('project-1'),
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2'),
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
      rootDir: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'project-2',
        version: '1.0.0',
      },
      rootDir: path.resolve('project-2'),
    },
  ]
  await mutateModules(importers,
    await testDefaults({ allProjects, hoistPattern: ['*'] })
  )

  {
    const modulesInfo = await readYamlFile<{ skipped: string[] }>(path.join('node_modules', '.modules.yaml'))
    expect(modulesInfo.skipped).toStrictEqual(['/@pnpm.e2e/dep-of-optional-pkg/1.0.0', '/@pnpm.e2e/not-compatible-with-any-os/1.0.0'])

    const currentLockfile = await readYamlFile<Lockfile>(path.resolve('node_modules/.pnpm/lock.yaml'))
    expect(currentLockfile.packages).toHaveProperty(['/@pnpm.e2e/not-compatible-with-any-os/1.0.0'])
  }

  await mutateModules([
    {
      ...importers[0],
      dependencySelectors: ['is-positive@1.0.0'],
      mutation: 'installSome',
    },
  ], await testDefaults({ allProjects, fastUnpack: false, hoistPattern: ['*'] }))

  {
    const currentLockfile = await readYamlFile<Lockfile>(path.resolve('node_modules/.pnpm/lock.yaml'))
    expect(currentLockfile.packages).toHaveProperty(['/@pnpm.e2e/not-compatible-with-any-os/1.0.0'])
  }
})

test('optional subdependency is skipped', async () => {
  const project = prepareEmpty()
  const reporter = sinon.spy()

  const manifest = await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-optional', '@pnpm.e2e/dep-of-optional-pkg'], await testDefaults({ reporter }))

  {
    const modulesInfo = await readYamlFile<{ skipped: string[] }>(path.join('node_modules', '.modules.yaml'))
    expect(modulesInfo.skipped).toStrictEqual(['/@pnpm.e2e/not-compatible-with-any-os/1.0.0'])
  }

  expect(await exists('node_modules/.pnpm/@pnpm.e2e+pkg-with-optional@1.0.0')).toBeTruthy()
  expect(await exists('node_modules/.pnpm/@pnpm.e2e+not-compatible-with-any-os@1.0.0')).toBeFalsy()

  const logMatcher = sinon.match({
    package: {
      id: `localhost+${REGISTRY_MOCK_PORT}/@pnpm.e2e/not-compatible-with-any-os/1.0.0`,
      name: '@pnpm.e2e/not-compatible-with-any-os',
      version: '1.0.0',
    },
    reason: 'unsupported_platform',
  })
  const reportedTimes = reporter.withArgs(logMatcher).callCount
  expect(reportedTimes).toBe(1)

  // recreate the lockfile with optional dependencies present

  expect(await exists('pnpm-lock.yaml')).toBeTruthy()
  await rimraf('pnpm-lock.yaml')

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd(),
  }, await testDefaults()
  )

  const lockfile = await project.readLockfile()

  expect(Object.keys(lockfile.packages).length).toBe(3)
  expect(lockfile.packages).toHaveProperty(['/@pnpm.e2e/not-compatible-with-any-os/1.0.0'])

  // forced headless install should install non-compatible optional deps

  // TODO: move next case to @pnpm/headless tests
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd(),
  }, await testDefaults({ force: true, frozenLockfile: true }))

  expect(await exists('node_modules/.pnpm/@pnpm.e2e+not-compatible-with-any-os@1.0.0')).toBeTruthy()

  {
    const modulesInfo = await readYamlFile<{ skipped: string[] }>(path.join('node_modules', '.modules.yaml'))
    expect(modulesInfo.skipped).toStrictEqual([])
  }
})

// Covers https://github.com/pnpm/pnpm/issues/2663
test('optional subdependency of newly added optional dependency is skipped', async () => {
  const project = prepareEmpty()
  const reporter = sinon.spy()

  await addDependenciesToPackage({}, ['@pnpm.e2e/pkg-with-optional'], await testDefaults({ reporter, targetDependenciesField: 'optionalDependencies' }))

  const modulesInfo = await readYamlFile<{ skipped: string[] }>(path.join('node_modules', '.modules.yaml'))
  expect(modulesInfo.skipped).toStrictEqual(['/@pnpm.e2e/dep-of-optional-pkg/1.0.0', '/@pnpm.e2e/not-compatible-with-any-os/1.0.0'])

  const lockfile = await project.readLockfile()

  expect(Object.keys(lockfile.packages).length).toBe(3)
  expect(lockfile.packages).toHaveProperty(['/@pnpm.e2e/not-compatible-with-any-os/1.0.0'])
})

test('only that package is skipped which is an optional dependency only and not installable', async () => {
  const project = prepareEmpty()
  const reporter = sinon.spy()

  const manifest = await addDependenciesToPackage({}, [
    '@pnpm.e2e/peer-c@1.0.0',
    '@pnpm.e2e/has-optional-dep-with-peer',
    '@pnpm.e2e/not-compatible-with-any-os-and-has-peer',
  ], await testDefaults({ reporter }))

  {
    const modulesInfo = await readYamlFile<{ skipped: string[] }>(path.join('node_modules', '.modules.yaml'))
    expect(modulesInfo.skipped).toStrictEqual([])
  }

  const lockfile = await project.readLockfile()
  expect(typeof lockfile.packages['/@pnpm.e2e/dep-of-optional-pkg/1.0.0'].optional).toBe('undefined')

  await rimraf('node_modules')

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd(),
  }, await testDefaults({ frozenLockfile: true }))

  {
    const modulesInfo = await readYamlFile<{ skipped: string[] }>(path.join('node_modules', '.modules.yaml'))
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
    await testDefaults({
      include: {
        dependencies: true,
        devDependencies: true,
        optionalDependencies: false,
      },
    })
  )

  await project.hasNot('is-positive')
  await project.has('@pnpm.e2e/pkg-with-good-optional')

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
  }, await testDefaults())

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
  }, await testDefaults({ fastUnpack: false, preferredVersions }))

  expect(await exists(path.resolve('node_modules/.pnpm/duplexify@3.6.0'))).toBeTruthy()
  expect(await exists(path.resolve('node_modules/.pnpm/stream-shift@1.0.0'))).toBeTruthy()

  expect(await exists(path.resolve('node_modules/.pnpm/got@3.3.1/node_modules/duplexify'))).toBeTruthy()
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

  const [{ manifest }] = await mutateModules(
    [
      {
        mutation: 'install',
        rootDir: path.resolve('project1'),
      },
      {
        mutation: 'install',
        rootDir: path.resolve('project2'),
      },
    ],
    await testDefaults({
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
          rootDir: path.resolve('project1'),
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
          rootDir: path.resolve('project2'),
        },
      ],
      lockfileDir: process.cwd(),
      lockfileOnly: true,
    })
  )

  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: path.resolve('project1'),
  }, await testDefaults({
    frozenLockfile: false,
    lockfileDir: process.cwd(),
    preferFrozenLockfile: false,
  }))

  const modulesInfo = await readYamlFile<{ skipped: string[] }>(path.join('node_modules', '.modules.yaml'))
  expect(modulesInfo.skipped).toStrictEqual([
    '/@pnpm.e2e/dep-of-optional-pkg/1.0.0',
    '/@pnpm.e2e/not-compatible-with-any-os/1.0.0',
  ])
})

test('do not fail on unsupported dependency of optional dependency', async () => {
  const project = prepareEmpty()

  await addDependenciesToPackage({}, ['@pnpm.e2e/not-compatible-with-not-compatible-dep@1.0.0'],
    await testDefaults({ targetDependenciesField: 'optionalDependencies' }, {}, {}, { engineStrict: true })
  )

  const lockfile = await project.readLockfile()
  expect(lockfile.packages['/@pnpm.e2e/not-compatible-with-any-os/1.0.0'].optional).toBeTruthy()
  expect(lockfile.packages['/@pnpm.e2e/dep-of-optional-pkg/1.0.0']).toBeTruthy()
})

test('fail on unsupported dependency of optional dependency', async () => {
  prepareEmpty()
  await expect(
    addDependenciesToPackage(
      {},
      ['@pnpm.e2e/has-not-compatible-dep@1.0.0'],
      await testDefaults({ targetDependenciesField: 'optionalDependencies' }, {}, {}, { engineStrict: true })
    )
  ).rejects.toThrow()
})
