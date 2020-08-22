import { WANTED_LOCKFILE } from '@pnpm/constants'
import { Lockfile } from '@pnpm/lockfile-file'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import { REGISTRY_MOCK_PORT } from '@pnpm/registry-mock'
import readYamlFile from 'read-yaml-file'
import {
  addDependenciesToPackage,
  install,
  MutatedProject,
  mutateModules,
} from 'supi'
import promisifyTape from 'tape-promise'
import { testDefaults } from '../utils'
import path = require('path')
import rimraf = require('@zkochan/rimraf')
import deepRequireCwd = require('deep-require-cwd')
import exists = require('path-exists')
import sinon = require('sinon')
import tape = require('tape')

const test = promisifyTape(tape)

test('successfully install optional dependency with subdependencies', async (t) => {
  prepareEmpty(t)

  await addDependenciesToPackage({}, ['fsevents@1.0.14'], await testDefaults({ targetDependenciesField: 'optionalDependencies' }))
})

test('skip failing optional dependencies', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  await addDependenciesToPackage({}, ['pkg-with-failing-optional-dependency@1.0.1'], await testDefaults({ fastUnpack: false }))

  const m = project.requireModule('pkg-with-failing-optional-dependency')
  t.ok(m(-1), 'package with failed optional dependency has the dependencies installed correctly')
})

test('skip non-existing optional dependency', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  const reporter = sinon.spy()
  await install({
    dependencies: {
      'is-positive': '*',
    },
    optionalDependencies: {
      'i-do-not-exist': '1000',
    },
  }, await testDefaults({ reporter }))

  t.ok(reporter.calledWithMatch({
    package: {
      name: 'i-do-not-exist',
      version: '1000',
    },
    parents: [],
    reason: 'resolution_failure',
  }), 'warning reported')

  await project.has('is-positive')

  const lockfile = await project.readLockfile()

  t.deepEqual(lockfile.specifiers, { 'is-positive': '*' }, `skipped optional dep not added to ${WANTED_LOCKFILE}`)
})

test('skip optional dependency that does not support the current OS', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  const reporter = sinon.spy()

  let manifest = await install({
    optionalDependencies: {
      'not-compatible-with-any-os': '*',
    },
  }, await testDefaults({ reporter }))

  await project.hasNot('not-compatible-with-any-os')
  await project.storeHas('not-compatible-with-any-os', '1.0.0')
  t.notOk(await exists(path.resolve('node_modules/.pnpm/dep-of-optional-pkg@1.0.0')), "isn't linked into node_modules")

  const lockfile = await project.readLockfile()
  t.ok(lockfile.packages['/not-compatible-with-any-os/1.0.0'], 'lockfile contains optional dependency')
  t.ok(lockfile.packages['/dep-of-optional-pkg/1.0.0'], 'lockfile contains dependency of optional dependency')

  const currentLockfile = await project.readCurrentLockfile()

  t.deepEqual(currentLockfile.packages, lockfile.packages, 'current lockfile contains skipped packages')

  const modulesInfo = await readYamlFile<{skipped: string[]}>(path.join('node_modules', '.modules.yaml'))
  t.deepEquals(modulesInfo.skipped, [
    '/dep-of-optional-pkg/1.0.0',
    '/not-compatible-with-any-os/1.0.0',
  ])

  const logMatcher = sinon.match({
    package: {
      id: `localhost+${REGISTRY_MOCK_PORT}/not-compatible-with-any-os/1.0.0`,
      name: 'not-compatible-with-any-os',
      version: '1.0.0',
    },
    reason: 'unsupported_platform',
  })
  const reportedTimes = reporter.withArgs(logMatcher).callCount
  t.equal(reportedTimes, 1, 'skipping optional dependency is logged')

  t.comment('a previously skipped package is successfully installed')

  manifest = await addDependenciesToPackage(manifest, ['dep-of-optional-pkg'], await testDefaults())

  await project.has('dep-of-optional-pkg')

  {
    const modules = await project.readModulesManifest()
    t.deepEqual(modules?.skipped, ['/not-compatible-with-any-os/1.0.0'], 'correct list of skipped packages')
  }

  await rimraf('node_modules')

  await mutateModules(
    [
      {
        buildIndex: 0,
        manifest,
        mutation: 'install',
        rootDir: process.cwd(),
      },
    ],
    await testDefaults({ frozenLockfile: true })
  )

  await project.hasNot('not-compatible-with-any-os')
  await project.has('dep-of-optional-pkg')

  {
    const modules = await project.readModulesManifest()
    t.deepEqual(modules?.skipped, ['/not-compatible-with-any-os/1.0.0'], 'correct list of skipped packages')
  }
})

test('skip optional dependency that does not support the current Node version', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  const reporter = sinon.spy()

  await install({
    optionalDependencies: {
      'for-legacy-node': '*',
    },
  }, await testDefaults({ reporter }))

  await project.hasNot('for-legacy-node')
  await project.storeHas('for-legacy-node', '1.0.0')

  const logMatcher = sinon.match({
    package: {
      id: `localhost+${REGISTRY_MOCK_PORT}/for-legacy-node/1.0.0`,
      name: 'for-legacy-node',
      version: '1.0.0',
    },
    reason: 'unsupported_engine',
  })
  const reportedTimes = reporter.withArgs(logMatcher).callCount
  t.equal(reportedTimes, 1, 'skipping optional dependency is logged')
})

test('skip optional dependency that does not support the current pnpm version', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  const reporter = sinon.spy()

  await install({
    optionalDependencies: {
      'for-legacy-pnpm': '*',
    },
  }, await testDefaults({
    packageManager: {
      name: 'pnpm',
      version: '4.0.0',
    },
    reporter,
  }))

  await project.hasNot('for-legacy-pnpm')
  await project.storeHas('for-legacy-pnpm', '1.0.0')

  const logMatcher = sinon.match({
    package: {
      id: `localhost+${REGISTRY_MOCK_PORT}/for-legacy-pnpm/1.0.0`,
      name: 'for-legacy-pnpm',
      version: '1.0.0',
    },
    reason: 'unsupported_engine',
  })
  const reportedTimes = reporter.withArgs(logMatcher).callCount
  t.equal(reportedTimes, 1, 'skipping optional dependency is logged')
})

test('don\'t skip optional dependency that does not support the current OS when forcing', async (t) => {
  const project = prepareEmpty(t)

  await install({
    optionalDependencies: {
      'not-compatible-with-any-os': '*',
    },
  }, await testDefaults({
    force: true,
  }))

  await project.has('not-compatible-with-any-os')
  await project.storeHas('not-compatible-with-any-os', '1.0.0')
})

// Covers https://github.com/pnpm/pnpm/issues/2636
test('optional subdependency is not removed from current lockfile when new dependency added', async (t: tape.Test) => {
  preparePackages(t, [
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
      buildIndex: 0,
      manifest: {
        name: 'project-1',
        version: '1.0.0',

        dependencies: {
          'pkg-with-optional': '1.0.0',
        },
      },
      mutation: 'install',
      rootDir: path.resolve('project-1'),
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'project-2',
        version: '1.0.0',
      },
      mutation: 'install',
      rootDir: path.resolve('project-2'),
    },
  ]
  await mutateModules(importers,
    await testDefaults({ hoistPattern: ['*'] })
  )

  {
    const modulesInfo = await readYamlFile<{ skipped: string[] }>(path.join('node_modules', '.modules.yaml'))
    t.deepEqual(modulesInfo.skipped, ['/dep-of-optional-pkg/1.0.0', '/not-compatible-with-any-os/1.0.0'], 'optional subdep skipped')

    const currentLockfile = await readYamlFile<Lockfile>(path.resolve('node_modules/.pnpm/lock.yaml'))
    t.ok(currentLockfile.packages?.['/not-compatible-with-any-os/1.0.0'])
  }

  await mutateModules([
    {
      ...importers[0],
      dependencySelectors: ['is-positive@1.0.0'],
      mutation: 'installSome',
    },
  ], await testDefaults({ fastUnpack: false, hoistPattern: ['*'] }))

  {
    const currentLockfile = await readYamlFile<Lockfile>(path.resolve('node_modules/.pnpm/lock.yaml'))
    t.ok(currentLockfile.packages?.['/not-compatible-with-any-os/1.0.0'])
  }
})

test('optional subdependency is skipped', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  const reporter = sinon.spy()

  const manifest = await addDependenciesToPackage({}, ['pkg-with-optional', 'dep-of-optional-pkg'], await testDefaults({ reporter }))

  {
    const modulesInfo = await readYamlFile<{ skipped: string[] }>(path.join('node_modules', '.modules.yaml'))
    t.deepEqual(modulesInfo.skipped, ['/not-compatible-with-any-os/1.0.0'], 'optional subdep skipped')
  }

  t.ok(await exists('node_modules/.pnpm/pkg-with-optional@1.0.0'), 'regular dependency linked')
  t.notOk(await exists('node_modules/.pnpm/not-compatible-with-any-os@1.0.0'), 'optional dependency not linked')

  const logMatcher = sinon.match({
    package: {
      id: `localhost+${REGISTRY_MOCK_PORT}/not-compatible-with-any-os/1.0.0`,
      name: 'not-compatible-with-any-os',
      version: '1.0.0',
    },
    reason: 'unsupported_platform',
  })
  const reportedTimes = reporter.withArgs(logMatcher).callCount
  t.equal(reportedTimes, 1, 'skipping optional dependency is logged')

  t.comment('recreate the lockfile with optional dependencies present')

  t.ok(await exists('pnpm-lock.yaml'))
  await rimraf('pnpm-lock.yaml')

  await mutateModules(
    [
      {
        buildIndex: 0,
        manifest,
        mutation: 'install',
        rootDir: process.cwd(),
      },
    ],
    await testDefaults()
  )

  const lockfile = await project.readLockfile()

  t.equal(Object.keys(lockfile.packages).length, 3)
  t.ok(lockfile.packages['/not-compatible-with-any-os/1.0.0'])

  t.comment('forced headless install should install non-compatible optional deps')

  // TODO: move next case to @pnpm/headless tests
  await mutateModules(
    [
      {
        buildIndex: 0,
        manifest,
        mutation: 'install',
        rootDir: process.cwd(),
      },
    ],
    await testDefaults({ force: true, frozenLockfile: true })
  )

  t.ok(await exists('node_modules/.pnpm/not-compatible-with-any-os@1.0.0'), 'optional dependency linked after forced headless install')

  {
    const modulesInfo = await readYamlFile<{ skipped: string[] }>(path.join('node_modules', '.modules.yaml'))
    t.deepEqual(modulesInfo.skipped, [], 'optional subdep removed from skipped list')
  }
})

// Covers https://github.com/pnpm/pnpm/issues/2663
test('optional subdependency of newly added optional dependency is skipped', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  const reporter = sinon.spy()

  await addDependenciesToPackage({}, ['pkg-with-optional'], await testDefaults({ reporter, targetDependenciesField: 'optionalDependencies' }))

  const modulesInfo = await readYamlFile<{ skipped: string[] }>(path.join('node_modules', '.modules.yaml'))
  t.deepEqual(modulesInfo.skipped, ['/dep-of-optional-pkg/1.0.0', '/not-compatible-with-any-os/1.0.0'], 'optional subdep skipped')

  const lockfile = await project.readLockfile()

  t.equal(Object.keys(lockfile.packages).length, 3)
  t.ok(lockfile.packages['/not-compatible-with-any-os/1.0.0'])
})

test('only that package is skipped which is an optional dependency only and not installable', async (t) => {
  const project = prepareEmpty(t)
  const reporter = sinon.spy()

  const manifest = await addDependenciesToPackage({}, [
    'peer-c@1.0.1',
    'has-optional-dep-with-peer',
    'not-compatible-with-any-os-and-has-peer',
  ], await testDefaults({ reporter }))

  {
    const modulesInfo = await readYamlFile<{ skipped: string[] }>(path.join('node_modules', '.modules.yaml'))
    t.deepEqual(modulesInfo.skipped, ['/not-compatible-with-any-os-and-has-peer/1.0.0_peer-c@1.0.0'])
  }

  const lockfile = await project.readLockfile()
  t.equal(typeof lockfile.packages['/dep-of-optional-pkg/1.0.0'].optional, 'undefined')

  await rimraf('node_modules')

  await mutateModules(
    [
      {
        buildIndex: 0,
        manifest,
        mutation: 'install',
        rootDir: process.cwd(),
      },
    ],
    await testDefaults({
      frozenLockfile: true,
    })
  )

  {
    const modulesInfo = await readYamlFile<{ skipped: string[] }>(path.join('node_modules', '.modules.yaml'))
    t.deepEqual(modulesInfo.skipped, ['/not-compatible-with-any-os-and-has-peer/1.0.0_peer-c@1.0.0'])
  }
})

test('not installing optional dependencies when optional is false', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await install(
    {
      dependencies: {
        'pkg-with-good-optional': '*',
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
  await project.has('pkg-with-good-optional')

  t.ok(deepRequireCwd(['pkg-with-good-optional', 'dep-of-pkg-with-1-dep', './package.json']))
  t.notOk(deepRequireCwd.silent(['pkg-with-good-optional', 'is-positive', './package.json']), 'optional subdep not installed')
})

test('optional dependency has bigger priority than regular dependency', async (t: tape.Test) => {
  prepareEmpty(t)

  await install({
    dependencies: {
      'is-positive': '1.0.0',
    },
    optionalDependencies: {
      'is-positive': '3.1.0',
    },
  }, await testDefaults())

  t.ok(deepRequireCwd(['is-positive', './package.json']).version, '3.1.0')
})

// Covers https://github.com/pnpm/pnpm/issues/1386
// TODO: use smaller packages to cover the test case
test('only skip optional dependencies', async (t: tape.Test) => {
  /*
    @google-cloud/functions-emulator has various dependencies, one of them is duplexify.
    duplexify depends on stream-shift. As duplexify is a dependency of an optional dependency
    and @google-cloud/functions-emulator won't be installed, duplexify and stream-shift
    are marked as skipped.
    firebase-tools also depends on duplexify and stream-shift, through got@3.3.1.
    Make sure that duplexify and stream-shift are installed because they are needed
    by firebase-tools, even if they were marked as skipped earlier.
  */

  prepareEmpty(t)

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

  t.ok(await exists(path.resolve('node_modules/.pnpm/duplexify@3.6.0')), 'duplexify is linked into node_modules')
  t.ok(await exists(path.resolve('node_modules/.pnpm/stream-shift@1.0.0')), 'stream-shift is linked into node_modules')

  t.ok(await exists(path.resolve('node_modules/.pnpm/got@3.3.1/node_modules/duplexify')), 'duplexify is linked into node_modules of got')
})

test('skip optional dependency that does not support the current OS, when doing install on a subset of workspace projects', async (t: tape.Test) => {
  preparePackages(t, [
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
        buildIndex: 0,
        manifest: {
          name: 'project1',
          version: '1.0.0',

          optionalDependencies: {
            'not-compatible-with-any-os': '*',
          },
        },
        mutation: 'install',
        rootDir: path.resolve('project1'),
      },
      {
        buildIndex: 0,
        manifest: {
          name: 'project2',
          version: '1.0.0',

          dependencies: {
            'pkg-with-1-dep': '100.0.0',
          },
        },
        mutation: 'install',
        rootDir: path.resolve('project2'),
      },
    ],
    await testDefaults({
      lockfileDir: process.cwd(),
      lockfileOnly: true,
    })
  )

  await mutateModules(
    [
      {
        buildIndex: 0,
        manifest,
        mutation: 'install',
        rootDir: path.resolve('project1'),
      },
    ],
    await testDefaults({
      frozenLockfile: false,
      lockfileDir: process.cwd(),
      preferFrozenLockfile: false,
    })
  )

  const modulesInfo = await readYamlFile<{skipped: string[]}>(path.join('node_modules', '.modules.yaml'))
  t.deepEquals(modulesInfo.skipped, [
    '/dep-of-optional-pkg/1.0.0',
    '/not-compatible-with-any-os/1.0.0',
  ])
})
