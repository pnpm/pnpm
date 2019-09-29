import { WANTED_LOCKFILE } from '@pnpm/constants'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import rimraf = require('@zkochan/rimraf')
import deepRequireCwd = require('deep-require-cwd')
import path = require('path')
import exists = require('path-exists')
import R = require('ramda')
import readYamlFile from 'read-yaml-file'
import sinon = require('sinon')
import { addDependenciesToPackage, install, mutateModules, rebuild } from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { testDefaults } from '../utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('successfully install optional dependency with subdependencies', async (t) => {
  prepareEmpty(t)

  await addDependenciesToPackage({}, ['fsevents@1.0.14'], await testDefaults({ targetDependenciesField: 'optionalDependencies' }))
})

test('skip failing optional dependencies', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  await addDependenciesToPackage({}, ['pkg-with-failing-optional-dependency@1.0.1'], await testDefaults())

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

  const m = project.requireModule('is-positive')
  t.ok(m, 'installation succeded')

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
  t.notOk(await exists(path.resolve('node_modules/.pnpm/localhost+4873/dep-of-optional-pkg/1.0.0')), "isn't linked into node_modules")

  const lockfile = await project.readLockfile()
  t.ok(lockfile.packages['/not-compatible-with-any-os/1.0.0'], 'lockfile contains optional dependency')
  t.ok(lockfile.packages['/dep-of-optional-pkg/1.0.0'], 'lockfile contains dependency of optional dependency')

  const currentLockfile = await project.readCurrentLockfile()

  t.ok(R.isEmpty(currentLockfile.packages || {}), 'current lockfile does not contain skipped packages')

  const modulesInfo = await readYamlFile<{skipped: string[]}>(path.join('node_modules', '.modules.yaml'))
  t.deepEquals(modulesInfo.skipped, [
    '/dep-of-optional-pkg/1.0.0',
    '/not-compatible-with-any-os/1.0.0',
  ])

  const logMatcher = sinon.match({
    package: {
      id: 'localhost+4873/not-compatible-with-any-os/1.0.0',
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
    t.deepEqual(modules && modules.skipped, ['/not-compatible-with-any-os/1.0.0'], 'correct list of skipped packages')
  }

  await rimraf('node_modules')

  await mutateModules(
    [
      {
        buildIndex: 0,
        manifest,
        mutation: 'install',
        prefix: process.cwd(),
      },
    ],
    await testDefaults({ frozenLockfile: true }),
  )

  await project.hasNot('not-compatible-with-any-os')
  await project.has('dep-of-optional-pkg')

  {
    const modules = await project.readModulesManifest()
    t.deepEqual(modules && modules.skipped, ['/not-compatible-with-any-os/1.0.0'], 'correct list of skipped packages')
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
      id: 'localhost+4873/for-legacy-node/1.0.0',
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
  }, await testDefaults({ reporter }))

  await project.hasNot('for-legacy-pnpm')
  await project.storeHas('for-legacy-pnpm', '1.0.0')

  const logMatcher = sinon.match({
    package: {
      id: 'localhost+4873/for-legacy-pnpm/1.0.0',
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

test('optional subdependency is skipped', async (t: tape.Test) => {
  prepareEmpty(t)
  const reporter = sinon.spy()

  const manifest = await addDependenciesToPackage({}, ['pkg-with-optional', 'dep-of-optional-pkg'], await testDefaults({ reporter }))

  {
    const modulesInfo = await readYamlFile<{ skipped: string[] }>(path.join('node_modules', '.modules.yaml'))
    t.deepEqual(modulesInfo.skipped, ['/not-compatible-with-any-os/1.0.0'], 'optional subdep skipped')
  }

  t.ok(await exists('node_modules/.pnpm/localhost+4873/pkg-with-optional/1.0.0'), 'regular dependency linked')
  t.notOk(await exists('node_modules/.pnpm/localhost+4873/not-compatible-with-any-os/1.0.0'), 'optional dependency not linked')

  const logMatcher = sinon.match({
    package: {
      id: 'localhost+4873/not-compatible-with-any-os/1.0.0',
      name: 'not-compatible-with-any-os',
      version: '1.0.0',
    },
    reason: 'unsupported_platform',
  })
  const reportedTimes = reporter.withArgs(logMatcher).callCount
  t.equal(reportedTimes, 1, 'skipping optional dependency is logged')

  // TODO: move next case to @pnpm/headless tests
  await mutateModules(
    [
      {
        buildIndex: 0,
        manifest,
        mutation: 'install',
        prefix: process.cwd(),
      },
    ],
    await testDefaults({ force: true, frozenLockfile: true }),
  )

  t.ok(await exists('node_modules/.pnpm/localhost+4873/not-compatible-with-any-os/1.0.0'), 'optional dependency linked after forced headless install')

  {
    const modulesInfo = await readYamlFile<{ skipped: string[] }>(path.join('node_modules', '.modules.yaml'))
    t.deepEqual(modulesInfo.skipped, [], 'optional subdep removed from skipped list')
  }
})

test('only that package is skipped which is an optional dependency only and not installable', async (t) => {
  prepareEmpty(t)
  const reporter = sinon.spy()

  const manifest = await addDependenciesToPackage({}, ['peer-c@1.0.1', 'has-optional-dep-with-peer', 'not-compatible-with-any-os-and-has-peer'], await testDefaults({ reporter }))

  {
    const modulesInfo = await readYamlFile<{ skipped: string[] }>(path.join('node_modules', '.modules.yaml'))
    t.deepEqual(modulesInfo.skipped, ['/not-compatible-with-any-os-and-has-peer/1.0.0_peer-c@1.0.0'])
  }

  await rimraf('node_modules')

  await mutateModules(
    [
      {
        buildIndex: 0,
        manifest,
        mutation: 'install',
        prefix: process.cwd(),
      },
    ],
    await testDefaults({
      frozenLockfile: true,
    }),
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
    }),
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

  const preferVersion = (selector: string) => ({ selector, type: 'version' as 'version' })
  const preferredVersions = {
    'duplexify': preferVersion('3.6.0'),
    'got': preferVersion('3.3.1'),
    'stream-shift': preferVersion('1.0.0'),
  }
  await install({
    dependencies: {
      'firebase-tools': '4.2.1',
    },
    optionalDependencies: {
      '@google-cloud/functions-emulator': '1.0.0-beta.5',
    },
  }, await testDefaults({ preferredVersions }))

  t.ok(await exists(path.resolve('node_modules/.pnpm/localhost+4873/duplexify/3.6.0')), 'duplexify is linked into node_modules')
  t.ok(await exists(path.resolve('node_modules/.pnpm/localhost+4873/stream-shift/1.0.0')), 'stream-shift is linked into node_modules')

  t.ok(await exists(path.resolve('node_modules/.pnpm/localhost+4873/got/3.3.1/node_modules/duplexify')), 'duplexify is linked into node_modules of got')
})

test(`rebuild should not fail on incomplete ${WANTED_LOCKFILE}`, async (t: tape.Test) => {
  prepareEmpty(t)

  const manifest = await install({
    dependencies: {
      'pre-and-postinstall-scripts-example': '1.0.0',
    },
    optionalDependencies: {
      'not-compatible-with-any-os': '1.0.0',
    },
  }, await testDefaults({ ignoreScripts: true }))

  const reporter = sinon.spy()

  await rebuild([{
    buildIndex: 0,
    manifest,
    prefix: process.cwd(),
  }], await testDefaults({ pending: true, reporter }))

  t.ok(reporter.calledWithMatch({
    level: 'debug',
    message: `No entry for "/not-compatible-with-any-os/1.0.0" in ${WANTED_LOCKFILE}`,
    name: 'pnpm',
  }), 'missing package reported')
})

test('skip optional dependency that does not support the current OS, when doing install on a subset of workspace packages', async (t: tape.Test) => {
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
        prefix: path.resolve('project1'),
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
        prefix: path.resolve('project2'),
      },
    ],
    await testDefaults({
      lockfileDirectory: process.cwd(),
      lockfileOnly: true,
    }),
  )

  await mutateModules(
    [
      {
        buildIndex: 0,
        manifest,
        mutation: 'install',
        prefix: path.resolve('project1'),
      },
    ],
    await testDefaults({
      frozenLockfile: false,
      lockfileDirectory: process.cwd(),
      preferFrozenLockfile: false,
    }),
  )

  const modulesInfo = await readYamlFile<{skipped: string[]}>(path.join('node_modules', '.modules.yaml'))
  t.deepEquals(modulesInfo.skipped, [
    '/dep-of-optional-pkg/1.0.0',
    '/not-compatible-with-any-os/1.0.0',
  ])
})
