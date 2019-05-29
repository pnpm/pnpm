import {
  CURRENT_LOCKFILE,
  WANTED_LOCKFILE,
} from '@pnpm/constants'
import { RootLog } from '@pnpm/core-loggers'
import { Lockfile } from '@pnpm/lockfile-file'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import { fromDir as readPackageJsonFromDir } from '@pnpm/read-package-json'
import { PackageJson } from '@pnpm/types'
import makeDir = require('make-dir')
import path = require('path')
import exists = require('path-exists')
import { getIntegrity } from 'pnpm-registry-mock'
import R = require('ramda')
import readYamlFile from 'read-yaml-file'
import rimraf = require('rimraf-then')
import sinon = require('sinon')
import {
  addDependenciesToPackage,
  install,
  mutateModules,
  uninstall,
} from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import writeYamlFile = require('write-yaml-file')
import {
  addDistTag,
  testDefaults,
} from './utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)
test['skip'] = promisifyTape(tape.skip) // tslint:disable-line:no-string-literal

const LOCKFILE_WARN_LOG = {
  level: 'warn',
  message: `A ${WANTED_LOCKFILE} file exists. The current configuration prohibits to read or write a lockfile`,
  name: 'pnpm',
}

test('lockfile has correct format', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDependenciesToPackage({},
    [
      'pkg-with-1-dep',
      '@rstacruz/tap-spec@4.1.1',
      'kevva/is-negative#1d7e288222b53a0cab90a331f1865220ec29560c',
    ], await testDefaults({ save: true }))

  const modules = await project.readModulesManifest()
  t.ok(modules)
  t.equal(modules!.pendingBuilds.length, 0)

  const lockfile = await project.readLockfile()
  const id = '/pkg-with-1-dep/100.0.0'

  t.equal(lockfile.lockfileVersion, 5, 'correct lockfile version')

  t.ok(lockfile.specifiers, 'has specifiers field')
  t.ok(lockfile.dependencies, 'has dependencies field')
  t.equal(lockfile.dependencies['pkg-with-1-dep'], '100.0.0', 'has dependency resolved')
  t.ok(lockfile.dependencies['@rstacruz/tap-spec'], 'has scoped dependency resolved')
  t.ok(lockfile.dependencies['is-negative'].includes('/'), 'has not shortened tarball from the non-standard registry')

  t.ok(lockfile.packages, 'has packages field')
  t.ok(lockfile.packages[id], `has resolution for ${id}`)
  t.ok(lockfile.packages[id].dependencies, `has dependency resolutions for ${id}`)
  t.ok(lockfile.packages[id].dependencies['dep-of-pkg-with-1-dep'], `has dependency resolved for ${id}`)
  t.ok(lockfile.packages[id].resolution, `has resolution for ${id}`)
  t.ok(lockfile.packages[id].resolution.integrity, `has integrity for package in the default registry`)
  t.notOk(lockfile.packages[id].resolution.tarball, `has no tarball for package in the default registry`)

  const absDepPath = 'github.com/kevva/is-negative/1d7e288222b53a0cab90a331f1865220ec29560c'
  t.ok(lockfile.packages[absDepPath])
  t.ok(lockfile.packages[absDepPath].name, 'github-hosted package has name specified')
})

test('lockfile has dev deps even when installing for prod only', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await install({
    devDependencies: {
      'is-negative': '2.1.0',
    },
  }, await testDefaults({ production: true }))

  const lockfile = await project.readLockfile()
  const id = '/is-negative/2.1.0'

  t.ok(lockfile.devDependencies, 'has devDependencies field')

  t.equal(lockfile.devDependencies['is-negative'], '2.1.0', 'has dev dependency resolved')

  t.ok(lockfile.packages[id], `has resolution for ${id}`)
})

test('lockfile with scoped package', async (t: tape.Test) => {
  prepareEmpty(t)

  await writeYamlFile(WANTED_LOCKFILE, {
    dependencies: {
      '@types/semver': '5.3.31',
    },
    lockfileVersion: 5,
    packages: {
      '/@types/semver/5.3.31': {
        resolution: {
          integrity: 'sha1-uZnX2TX0P1IHsBsA094ghS9Mp18=',
        },
      },
    },
    specifiers: {
      '@types/semver': '^5.3.31',
    },
  })

  await install({
    dependencies: {
      '@types/semver': '^5.3.31',
    },
  }, await testDefaults({ frozenLockfile: true }))
})

test('fail when shasum from lockfile does not match with the actual one', async (t: tape.Test) => {
  prepareEmpty(t)

  await writeYamlFile(WANTED_LOCKFILE, {
    dependencies: {
      'is-negative': '2.1.0',
    },
    lockfileVersion: 5,
    packages: {
      '/is-negative/2.1.0': {
        resolution: {
          integrity: 'sha1-uZnX2TX0P1IHsBsA094ghS9Mp10=',
          tarball: 'http://localhost:4873/is-negative/-/is-negative-2.1.0.tgz',
        },
      },
    },
    specifiers: {
      'is-negative': '2.1.0',
    },
  })

  try {
    await install({
      dependencies: {
        'is-negative': '2.1.0',
      },
    }, await testDefaults())
    t.fail('installation should have failed')
  } catch (err) {
    t.equal(err.code, 'EINTEGRITY')
  }
})

test("lockfile doesn't lock subdependencies that don't satisfy the new specs", async (t: tape.Test) => {
  const project = prepareEmpty(t)

  // dependends on react-onclickoutside@5.9.0
  let manifest = await addDependenciesToPackage({}, ['react-datetime@2.8.8'], await testDefaults({ save: true }))

  // dependends on react-onclickoutside@0.3.4
  await addDependenciesToPackage(manifest, ['react-datetime@1.3.0'], await testDefaults({ save: true }))

  t.equal(
    project.requireModule('.localhost+4873/react-datetime/1.3.0/node_modules/react-onclickoutside/package.json').version,
    '0.3.4',
    'react-datetime@1.3.0 has react-onclickoutside@0.3.4 in its node_modules')

  const lockfile = await project.readLockfile()

  t.equal(Object.keys(lockfile.dependencies).length, 1, 'resolutions not duplicated')
})

test('lockfile not created when no deps in package.json', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await install({}, await testDefaults())

  t.notOk(await project.readLockfile(), 'lockfile not created')
  t.notOk(await exists('node_modules'), 'empty node_modules not created')
})

test('lockfile removed when no deps in package.json', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await writeYamlFile(WANTED_LOCKFILE, {
    dependencies: {
      'is-negative': '2.1.0',
    },
    lockfileVersion: 5,
    packages: {
      '/is-negative/2.1.0': {
        resolution: {
          tarball: 'http://localhost:4873/is-negative/-/is-negative-2.1.0.tgz',
        },
      },
    },
    specifiers: {
      'is-negative': '2.1.0',
    },
  })

  await install({}, await testDefaults())

  t.notOk(await project.readLockfile(), 'lockfile removed')
})

test('lockfile is fixed when it does not match package.json', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await writeYamlFile(WANTED_LOCKFILE, {
    dependencies: {
      '@types/semver': '5.3.31',
      'is-negative': '2.1.0',
      'is-positive': '3.1.0',
    },
    lockfileVersion: 5,
    packages: {
      '/@types/semver/5.3.31': {
        resolution: {
          integrity: 'sha1-uZnX2TX0P1IHsBsA094ghS9Mp18=',
        },
      },
      '/is-negative/2.1.0': {
        resolution: {
          tarball: 'http://localhost:4873/is-negative/-/is-negative-2.1.0.tgz',
        },
      },
      '/is-positive/3.1.0': {
        resolution: {
          integrity: 'sha1-hX21hKG6XRyymAUn/DtsQ103sP0=',
        },
      },
    },
    specifiers: {
      '@types/semver': '5.3.31',
      'is-negative': '^2.1.0',
      'is-positive': '^3.1.0',
    },
  })

  const reporter = sinon.spy()
  await install({
    devDependencies: {
      'is-negative': '^2.1.0',
    },
    optionalDependencies: {
      'is-positive': '^3.1.0',
    },
  }, await testDefaults({ reporter }))

  const progress = sinon.match({
    name: 'pnpm:progress',
    status: 'resolving',
  })
  t.equal(reporter.withArgs(progress).callCount, 0, 'resolving not reported')

  const lockfile = await project.readLockfile()

  t.equal(lockfile.devDependencies['is-negative'], '2.1.0', `is-negative moved to devDependencies in ${WANTED_LOCKFILE}`)
  t.equal(lockfile.optionalDependencies['is-positive'], '3.1.0', `is-positive moved to optionalDependencies in ${WANTED_LOCKFILE}`)
  t.notOk(lockfile.dependencies, 'empty dependencies property removed')
  t.notOk(lockfile.packages['/@types/semver/5.3.31'], 'package not referenced in package.json removed')
})

test(`doing named installation when ${WANTED_LOCKFILE} exists already`, async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await writeYamlFile(WANTED_LOCKFILE, {
    dependencies: {
      '@types/semver': '5.3.31',
      'is-negative': '2.1.0',
      'is-positive': '3.1.0',
    },
    lockfileVersion: 5,
    packages: {
      '/@types/semver/5.3.31': {
        resolution: {
          integrity: 'sha1-uZnX2TX0P1IHsBsA094ghS9Mp18=',
        },
      },
      '/is-negative/2.1.0': {
        resolution: {
          tarball: 'http://localhost:4873/is-negative/-/is-negative-2.1.0.tgz',
        },
      },
      '/is-positive/3.1.0': {
        resolution: {
          integrity: 'sha1-hX21hKG6XRyymAUn/DtsQ103sP0=',
        },
      },
    },
    specifiers: {
      '@types/semver': '5.3.31',
      'is-negative': '^2.1.0',
      'is-positive': '^3.1.0',
    },
  })

  const reporter = sinon.spy()

  const manifest = await addDependenciesToPackage({
    dependencies: {
      '@types/semver': '5.3.31',
      'is-negative': '^2.1.0',
      'is-positive': '^3.1.0',
    },
  }, ['is-positive'], await testDefaults({ reporter }))
  await install(manifest, await testDefaults({ reporter }))

  t.notOk(reporter.calledWithMatch(LOCKFILE_WARN_LOG), `no warning about ignoring ${WANTED_LOCKFILE}`)

  await project.has('is-negative')
})

test(`respects ${WANTED_LOCKFILE} for top dependencies`, async (t: tape.Test) => {
  const project = prepareEmpty(t)
  const reporter = sinon.spy()
  // const fooProgress = sinon.match({
  //   name: 'pnpm:progress',
  //   status: 'resolving',
  //   manifest: {
  //     name: 'foo',
  //   },
  // })

  const pkgs = ['foo', 'bar', 'qar']
  await Promise.all(pkgs.map((pkgName) => addDistTag(pkgName, '100.0.0', 'latest')))

  let manifest = await addDependenciesToPackage({}, ['foo'], await testDefaults({ save: true, reporter }))
  // t.equal(reporter.withArgs(fooProgress).callCount, 1, 'reported foo once')
  manifest = await addDependenciesToPackage(manifest, ['bar'], await testDefaults({ targetDependenciesField: 'optionalDependencies' }))
  manifest = await addDependenciesToPackage(manifest, ['qar'], await testDefaults({ addDependenciesToPackage: 'devDependencies' }))
  manifest = await addDependenciesToPackage(manifest, ['foobar'], await testDefaults({ save: true }))

  t.equal((await readPackageJsonFromDir(path.resolve('node_modules', 'foo'))).version, '100.0.0')
  t.equal((await readPackageJsonFromDir(path.resolve('node_modules', 'bar'))).version, '100.0.0')
  t.equal((await readPackageJsonFromDir(path.resolve('node_modules', 'qar'))).version, '100.0.0')
  t.equal((await readPackageJsonFromDir(path.resolve('node_modules', '.localhost+4873', 'foobar', '100.0.0', 'node_modules', 'foo'))).version, '100.0.0')
  t.equal((await readPackageJsonFromDir(path.resolve('node_modules', '.localhost+4873', 'foobar', '100.0.0', 'node_modules', 'bar'))).version, '100.0.0')

  await Promise.all(pkgs.map((pkgName) => addDistTag(pkgName, '100.1.0', 'latest')))

  await rimraf('node_modules')
  await rimraf(path.join('..', '.store'))

  reporter.resetHistory()

  // shouldn't care about what the registry in npmrc is
  // the one in lockfile should be used
  await install(manifest, await testDefaults({
    rawNpmConfig: {
      registry: 'https://registry.npmjs.org',
    },
    registry: 'https://registry.npmjs.org',
    reporter,
  }))

  // t.equal(reporter.withArgs(fooProgress).callCount, 0, 'not reported foo')

  await project.storeHasNot('foo', '100.1.0')
  t.equal((await readPackageJsonFromDir(path.resolve('node_modules', 'foo'))).version, '100.0.0')
  t.equal((await readPackageJsonFromDir(path.resolve('node_modules', 'bar'))).version, '100.0.0')
  t.equal((await readPackageJsonFromDir(path.resolve('node_modules', 'qar'))).version, '100.0.0')
  t.equal((await readPackageJsonFromDir(path.resolve('node_modules', '.localhost+4873', 'foobar', '100.0.0', 'node_modules', 'foo'))).version, '100.0.0')
  t.equal((await readPackageJsonFromDir(path.resolve('node_modules', '.localhost+4873', 'foobar', '100.0.0', 'node_modules', 'bar'))).version, '100.0.0')
})

test(`subdeps are updated on repeat install if outer ${WANTED_LOCKFILE} does not match the inner one`, async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'latest')

  const manifest = await addDependenciesToPackage({}, ['pkg-with-1-dep'], await testDefaults())

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  const lockfile = await project.readLockfile()

  t.ok(lockfile.packages['/dep-of-pkg-with-1-dep/100.0.0'])

  delete lockfile.packages['/dep-of-pkg-with-1-dep/100.0.0']

  lockfile.packages['/dep-of-pkg-with-1-dep/100.1.0'] = {
    resolution: {
      integrity: getIntegrity('dep-of-pkg-with-1-dep', '100.1.0'),
    },
  }

  lockfile.packages['/pkg-with-1-dep/100.0.0'].dependencies['dep-of-pkg-with-1-dep'] = '100.1.0'

  await writeYamlFile(WANTED_LOCKFILE, lockfile)

  await install(manifest, await testDefaults())

  await project.storeHas('dep-of-pkg-with-1-dep', '100.1.0')
})

test("recreates lockfile if it doesn't match the dependencies in package.json", async (t: tape.Test) => {
  const project = prepareEmpty(t)

  let manifest = await addDependenciesToPackage({}, ['is-negative@1.0.0'], await testDefaults({ pinnedVersion: 'patch', targetDependenciesField: 'dependencies' }))
  manifest = await addDependenciesToPackage(manifest, ['is-positive@1.0.0'], await testDefaults({ pinnedVersion: 'patch', targetDependenciesField: 'devDependencies' }))
  manifest = await addDependenciesToPackage(manifest, ['map-obj@1.0.0'], await testDefaults({ pinnedVersion: 'patch', targetDependenciesField: 'optionalDependencies' }))

  const lockfile1 = await project.readLockfile()
  t.equal(lockfile1.dependencies['is-negative'], '1.0.0')
  t.equal(lockfile1.specifiers['is-negative'], '1.0.0')

  manifest.dependencies!['is-negative'] = '^2.1.0'
  manifest.devDependencies!['is-positive'] = '^2.0.0'
  manifest.optionalDependencies!['map-obj'] = '1.0.1'

  await install(manifest, await testDefaults())

  const lockfile = await project.readLockfile()

  t.equal(lockfile.dependencies['is-negative'], '2.1.0')
  t.equal(lockfile.specifiers['is-negative'], '^2.1.0')

  t.equal(lockfile.devDependencies['is-positive'], '2.0.0')
  t.equal(lockfile.specifiers['is-positive'], '^2.0.0')

  t.equal(lockfile.optionalDependencies['map-obj'], '1.0.1')
  t.equal(lockfile.specifiers['map-obj'], '1.0.1')
})

test('repeat install with lockfile should not mutate lockfile when dependency has version specified with v prefix', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  const manifest = await addDependenciesToPackage({}, ['highmaps-release@5.0.11'], await testDefaults())

  const lockfile1 = await project.readLockfile()

  t.equal(lockfile1.dependencies['highmaps-release'], '5.0.11', `dependency added correctly to ${WANTED_LOCKFILE}`)

  await rimraf('node_modules')

  await install(manifest, await testDefaults())

  const lockfile2 = await project.readLockfile()

  t.deepEqual(lockfile1, lockfile2, "lockfile hasn't been changed")
})

test('package is not marked dev if it is also a subdep of a regular dependency', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'latest')

  const manifest = await addDependenciesToPackage({}, ['pkg-with-1-dep'], await testDefaults())

  t.pass('installed pkg-with-1-dep')

  await addDependenciesToPackage(manifest, ['dep-of-pkg-with-1-dep'], await testDefaults({ targetDependenciesField: 'devDependencies' }))

  t.pass('installed optional dependency which is also a dependency of pkg-with-1-dep')

  const lockfile = await project.readLockfile()

  t.notOk(lockfile.packages['/dep-of-pkg-with-1-dep/100.0.0'].dev, 'package is not marked as dev')
})

test('package is not marked optional if it is also a subdep of a regular dependency', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'latest')

  const manifest = await addDependenciesToPackage({}, ['pkg-with-1-dep'], await testDefaults())
  await addDependenciesToPackage(manifest, ['dep-of-pkg-with-1-dep'], await testDefaults({ targetDependenciesField: 'optionalDependencies' }))

  const lockfile = await project.readLockfile()

  t.notOk(lockfile.packages['/dep-of-pkg-with-1-dep/100.0.0'].optional, 'package is not marked as optional')
})

test('scoped module from different registry', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  const opts = await testDefaults()
  opts.registries!.default = 'https://registry.npmjs.org/' // tslint:disable-line
  opts.registries!['@zkochan'] = 'http://localhost:4873' // tslint:disable-line
  opts.registries!['@foo'] = 'http://localhost:4873' // tslint:disable-line
  await addDependenciesToPackage({}, ['@zkochan/foo', '@foo/has-dep-from-same-scope', 'is-positive'], opts)

  const m = project.requireModule('@zkochan/foo')
  t.ok(m, 'foo is available')

  const lockfile = await project.readLockfile()

  t.deepEqual(lockfile, {
    dependencies: {
      '@foo/has-dep-from-same-scope': '1.0.0',
      '@zkochan/foo': '1.0.0',
      'is-positive': '3.1.0',
    },
    lockfileVersion: 5,
    packages: {
      '/@foo/has-dep-from-same-scope/1.0.0': {
        dependencies: {
          '@foo/no-deps': '1.0.0',
          'is-negative': '1.0.0',
        },
        dev: false,
        resolution: {
          integrity: getIntegrity('@foo/has-dep-from-same-scope', '1.0.0'),
        },
      },
      '/@foo/no-deps/1.0.0': {
        dev: false,
        resolution: {
          integrity: getIntegrity('@foo/no-deps', '1.0.0'),
        },
      },
      '/@zkochan/foo/1.0.0': {
        dev: false,
        resolution: {
          integrity: 'sha512-IFvrYpq7E6BqKex7A7czIFnFncPiUVdhSzGhAOWpp8RlkXns4y/9ZdynxaA/e0VkihRxQkihE2pTyvxjfe/wBg==',
        },
      },
      '/is-negative/1.0.0': {
        dev: false,
        engines: {
          node: '>=0.10.0',
        },
        resolution: {
          integrity: 'sha1-clmHeoPIAKwxkd17nZ+80PdS1P4=',
        },
      },
      '/is-positive/3.1.0': {
        dev: false,
        engines: {
          node: '>=0.10.0',
        },
        resolution: {
          integrity: 'sha1-hX21hKG6XRyymAUn/DtsQ103sP0=',
        },
      },
    },
    specifiers: {
      '@foo/has-dep-from-same-scope': '^1.0.0',
      '@zkochan/foo': '^1.0.0',
      'is-positive': '^3.1.0',
    },
  })
})

test('repeat install with no inner lockfile should not rewrite packages in node_modules', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  const manifest = await addDependenciesToPackage({}, ['is-negative@1.0.0'], await testDefaults())

  await rimraf(CURRENT_LOCKFILE)

  await install(manifest, await testDefaults())

  const m = project.requireModule('is-negative')
  t.ok(m)
})

// Skipped because the npm-registry.compass.com server was down
// might be a good idea to mock it
// tslint:disable-next-line:no-string-literal
test['skip']('installing from lockfile when using npm enterprise', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  const opts = await testDefaults({ registry: 'https://npm-registry.compass.com/' })

  const manifest = await addDependenciesToPackage({}, ['is-positive@3.1.0'], opts)

  const lockfile = await project.readLockfile()

  t.deepEqual(lockfile, {
    dependencies: {
      'is-positive': '3.1.0',
    },
    lockfileVersion: 5,
    packages: {
      '/is-positive/3.1.0': {
        dev: false,
        engines: {
          node: '>=0.10.0',
        },
        resolution: {
          integrity: 'sha1-hX21hKG6XRyymAUn/DtsQ103sP0=',
          tarball: '/i/is-positive/_attachments/is-positive-3.1.0.tgz',
        },
      },
    },
    specifiers: {
      'is-positive': '^3.1.0',
    },
  })

  await rimraf(opts.store)
  await rimraf('node_modules')

  await install(manifest, opts)

  await project.has('is-positive')
})

test('packages are placed in devDependencies even if they are present as non-dev as well', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  const reporter = sinon.spy()
  await install({
    devDependencies: {
      'dep-of-pkg-with-1-dep': '^100.1.0',
      'pkg-with-1-dep': '^100.0.0',
    },
  }, await testDefaults({ reporter }))

  const lockfile = await project.readLockfile()

  t.ok(lockfile.devDependencies['dep-of-pkg-with-1-dep'])
  t.ok(lockfile.devDependencies['pkg-with-1-dep'])

  t.ok(reporter.calledWithMatch({
    added: {
      dependencyType: 'dev',
      name: 'dep-of-pkg-with-1-dep',
      version: '100.1.0',
    },
    level: 'debug',
    name: 'pnpm:root',
  } as RootLog), 'dep-of-pkg-with-1-dep added to root')
  t.ok(reporter.calledWithMatch({
    added: {
      dependencyType: 'dev',
      name: 'pkg-with-1-dep',
      version: '100.0.0',
    },
    level: 'debug',
    name: 'pnpm:root',
  } as RootLog), 'pkg-with-1-dep added to root')
})

// This testcase verifies that pnpm is not failing when trying to preserve dependencies.
// Only when a dependency is a range dependency, should pnpm try to compare versions of deps with semver.satisfies().
test('updating package that has a github-hosted dependency', async (t: tape.Test) => {
  prepareEmpty(t)

  const manifest = await addDependenciesToPackage({}, ['has-github-dep@1'], await testDefaults())
  await addDependenciesToPackage(manifest, ['has-github-dep@latest'], await testDefaults())

  t.pass('installation of latest did not fail')
})

test('updating package that has deps with peers', async (t: tape.Test) => {
  prepareEmpty(t)

  const manifest = await addDependenciesToPackage({}, ['abc-grand-parent-with-c@0'], await testDefaults())
  await addDependenciesToPackage(manifest, ['abc-grand-parent-with-c@1'], await testDefaults())

  t.pass('installation of latest did not fail')
})

test('pendingBuilds gets updated if install removes packages', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await install({
    dependencies: {
      'pre-and-postinstall-scripts-example': '*',
      'with-postinstall-b': '*',
    },
  }, await testDefaults({ ignoreScripts: true }))
  const modules1 = await project.readModulesManifest()

  await install({
    dependencies: {
      'pre-and-postinstall-scripts-example': '*',
    },
  }, await testDefaults({ ignoreScripts: true }))
  const modules2 = await project.readModulesManifest()

  t.ok(modules1)
  t.ok(modules2)
  t.ok(modules1!.pendingBuilds.length > modules2!.pendingBuilds.length, 'pendingBuilds gets updated when install removes packages')
})

test('dev properties are correctly updated on named install', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  const manifest = await addDependenciesToPackage(
    {},
    ['inflight@1.0.6'],
    await testDefaults({ targetDependenciesField: 'devDependencies' }),
  )
  await addDependenciesToPackage(manifest, ['foo@npm:inflight@1.0.6'], await testDefaults({}))

  const lockfile = await project.readLockfile()
  t.deepEqual(
    R.values(lockfile.packages).filter((dep) => typeof dep.dev !== 'undefined'),
    [],
    `there are 0 packages with dev property in ${WANTED_LOCKFILE}`,
  )
})

test('optional properties are correctly updated on named install', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  const manifest = await addDependenciesToPackage({}, ['inflight@1.0.6'], await testDefaults({ targetDependenciesField: 'optionalDependencies' }))
  await addDependenciesToPackage(manifest, ['foo@npm:inflight@1.0.6'], await testDefaults({}))

  const lockfile = await project.readLockfile()
  t.deepEqual(R.values(lockfile.packages).filter((dep) => typeof dep.optional !== 'undefined'), [], `there are 0 packages with optional property in ${WANTED_LOCKFILE}`)
})

test('dev property is correctly set for package that is duplicated to both the dependencies and devDependencies group', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  // TODO: use a smaller package for testing
  await addDependenciesToPackage({}, ['overlap@2.2.8'], await testDefaults())

  const lockfile = await project.readLockfile()
  t.ok(lockfile.packages['/couleurs/5.0.0'].dev === false)
})

test('no lockfile', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  const reporter = sinon.spy()

  await addDependenciesToPackage({}, ['is-positive'], await testDefaults({ useLockfile: false, reporter }))

  t.notOk(reporter.calledWithMatch(LOCKFILE_WARN_LOG), `no warning about ignoring ${WANTED_LOCKFILE}`)

  await project.has('is-positive')

  t.notOk(await project.readLockfile(), `${WANTED_LOCKFILE} not created`)
})

test('lockfile is ignored when lockfile = false', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await writeYamlFile(WANTED_LOCKFILE, {
    dependencies: {
      'is-negative': '2.1.0',
    },
    lockfileVersion: 5,
    packages: {
      '/is-negative/2.1.0': {
        resolution: {
          integrity: 'sha1-uZnX2TX0P1IHsBsA094ghS9Mp10=', // Invalid integrity
          tarball: 'http://localhost:4873/is-negative/-/is-negative-2.1.0.tgz',
        },
      },
    },
    specifiers: {
      'is-negative': '2.1.0',
    },
  })

  const reporter = sinon.spy()

  await install({
    dependencies: {
      'is-negative': '2.1.0',
    },
  }, await testDefaults({ useLockfile: false, reporter }))

  t.ok(reporter.calledWithMatch(LOCKFILE_WARN_LOG), `warning about ignoring ${WANTED_LOCKFILE}`)

  await project.has('is-negative')

  t.ok(await project.readLockfile(), `existing ${WANTED_LOCKFILE} not removed`)
})

test(`don't update ${WANTED_LOCKFILE} during uninstall when useLockfile: false`, async (t: tape.Test) => {
  const project = prepareEmpty(t)

  let manifest!: PackageJson
  {
    const reporter = sinon.spy()

    manifest = await addDependenciesToPackage({}, ['is-positive'], await testDefaults({ reporter }))

    t.notOk(reporter.calledWithMatch(LOCKFILE_WARN_LOG), `no warning about ignoring ${WANTED_LOCKFILE}`)
  }

  {
    const reporter = sinon.spy()

    await uninstall(manifest, ['is-positive'], await testDefaults({ useLockfile: false, reporter }))

    t.ok(reporter.calledWithMatch(LOCKFILE_WARN_LOG), `warning about ignoring ${WANTED_LOCKFILE}`)
  }

  await project.hasNot('is-positive')

  t.ok(await project.readLockfile(), `${WANTED_LOCKFILE} not removed during uninstall`)
})

test('fail when installing with useLockfile: false and lockfileOnly: true', async (t: tape.Test) => {
  prepareEmpty(t)

  try {
    await install({}, await testDefaults({ useLockfile: false, lockfileOnly: true }))
    t.fail('installation should have failed')
  } catch (err) {
    t.equal(err.message, `Cannot generate a ${WANTED_LOCKFILE} because lockfile is set to false`)
  }
})

test("don't remove packages during named install when useLockfile: false", async (t: tape.Test) => {
  const project = prepareEmpty(t)

  const manifest = await addDependenciesToPackage({}, ['is-positive'], await testDefaults({ useLockfile: false }))
  await addDependenciesToPackage(manifest, ['is-negative'], await testDefaults({ useLockfile: false }))

  await project.has('is-positive')
  await project.has('is-negative')
})

test('save tarball URL when it is non-standard', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDependenciesToPackage({}, ['esprima-fb@3001.1.0-dev-harmony-fb'], await testDefaults())

  const lockfile = await project.readLockfile()

  t.equal(lockfile.packages['/esprima-fb/3001.1.0-dev-harmony-fb'].resolution.tarball, 'esprima-fb/-/esprima-fb-3001.0001.0000-dev-harmony-fb.tgz')
})

test('packages installed via tarball URL from the default registry are normalized', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDependenciesToPackage({}, [
    'http://localhost:4873/pkg-with-tarball-dep-from-registry/-/pkg-with-tarball-dep-from-registry-1.0.0.tgz',
    'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
  ], await testDefaults())

  const lockfile = await project.readLockfile()

  t.deepEqual(lockfile, {
    dependencies: {
      'is-positive': 'registry.npmjs.org/is-positive/-/is-positive-1.0.0',
      'pkg-with-tarball-dep-from-registry': '1.0.0',
    },
    lockfileVersion: 5,
    packages: {
      '/dep-of-pkg-with-1-dep/100.0.0': {
        dev: false,
        resolution: {
          integrity: getIntegrity('dep-of-pkg-with-1-dep', '100.0.0'),
        },
      },
      '/pkg-with-tarball-dep-from-registry/1.0.0': {
        dependencies: {
          'dep-of-pkg-with-1-dep': '100.0.0',
        },
        dev: false,
        resolution: {
          integrity: getIntegrity('pkg-with-tarball-dep-from-registry', '1.0.0'),
        },
      },
      'registry.npmjs.org/is-positive/-/is-positive-1.0.0': {
        dev: false,
        engines: { node: '>=0.10.0' },
        name: 'is-positive',
        resolution: {
          tarball: 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
        },
        version: '1.0.0',
      },
    },
    specifiers: {
      'is-positive': 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
      'pkg-with-tarball-dep-from-registry': 'http://localhost:4873/pkg-with-tarball-dep-from-registry/-/pkg-with-tarball-dep-from-registry-1.0.0.tgz',
    },
  })
})

test('lockfile file has correct format when lockfile directory does not equal the prefix directory', async (t: tape.Test) => {
  prepareEmpty(t)

  const store = path.resolve('..', '.store')

  const manifest = await addDependenciesToPackage(
    {},
    [
      'pkg-with-1-dep',
      '@rstacruz/tap-spec@4.1.1',
      'kevva/is-negative#1d7e288222b53a0cab90a331f1865220ec29560c',
    ],
    await testDefaults({ save: true, lockfileDirectory: path.resolve('..'), store }),
  )

  t.ok(!await exists('node_modules/.modules.yaml'), ".modules.yaml in importer's node_modules not created")

  process.chdir('..')

  const modules = await readYamlFile<object>(path.resolve('node_modules', '.modules.yaml'))
  t.ok(modules, '.modules.yaml in virtual store directory created')
  t.equal(modules['pendingBuilds'].length, 0) // tslint:disable-line:no-string-literal

  {
    const lockfile = await readYamlFile(WANTED_LOCKFILE) as Lockfile
    const id = '/pkg-with-1-dep/100.0.0'

    t.equal(lockfile.lockfileVersion, 5, 'correct lockfile version')

    t.ok(lockfile.importers)
    t.ok(lockfile.importers.project)
    t.ok(lockfile.importers.project.specifiers, 'has specifiers field')
    t.ok(lockfile.importers.project.dependencies, 'has dependencies field')
    t.equal(lockfile.importers.project.dependencies!['pkg-with-1-dep'], '100.0.0', 'has dependency resolved')
    t.ok(lockfile.importers.project.dependencies!['@rstacruz/tap-spec'], 'has scoped dependency resolved')
    t.ok(lockfile.importers.project.dependencies!['is-negative'].includes('/'), 'has not shortened tarball from the non-standard registry')

    t.ok(lockfile.packages, 'has packages field')
    t.ok(lockfile.packages![id], `has resolution for ${id}`)
    t.ok(lockfile.packages![id].dependencies, `has dependency resolutions for ${id}`)
    t.ok(lockfile.packages![id].dependencies!['dep-of-pkg-with-1-dep'], `has dependency resolved for ${id}`)
    t.ok(lockfile.packages![id].resolution, `has resolution for ${id}`)
    t.ok(lockfile.packages![id].resolution['integrity'], `has integrity for package in the default registry`) // tslint:disable-line
    t.notOk(lockfile.packages![id].resolution['tarball'], `has no tarball for package in the default registry`) // tslint:disable-line

    const absDepPath = 'github.com/kevva/is-negative/1d7e288222b53a0cab90a331f1865220ec29560c'
    t.ok(lockfile.packages![absDepPath])
    t.ok(lockfile.packages![absDepPath].name, 'github-hosted package has name specified')
  }

  await makeDir('project-2')

  process.chdir('project-2')

  await addDependenciesToPackage(manifest, ['is-positive'], await testDefaults({ save: true, lockfileDirectory: path.resolve('..'), store }))

  {
    const lockfile = await readYamlFile<Lockfile>(path.join('..', WANTED_LOCKFILE))

    t.ok(lockfile.importers)
    t.ok(lockfile.importers['project-2'])

    // previous entries are not removed
    const id = '/pkg-with-1-dep/100.0.0'

    t.ok(lockfile.importers)
    t.ok(lockfile.importers.project)
    t.ok(lockfile.importers.project.specifiers, 'has specifiers field')
    t.ok(lockfile.importers.project.dependencies, 'has dependencies field')
    t.equal(lockfile.importers.project.dependencies!['pkg-with-1-dep'], '100.0.0', 'has dependency resolved')
    t.ok(lockfile.importers.project.dependencies!['@rstacruz/tap-spec'], 'has scoped dependency resolved')
    t.ok(lockfile.importers.project.dependencies!['is-negative'].includes('/'), 'has not shortened tarball from the non-standard registry')

    t.ok(lockfile.packages, 'has packages field')
    t.ok(lockfile.packages![id], `has resolution for ${id}`)
    t.ok(lockfile.packages![id].dependencies, `has dependency resolutions for ${id}`)
    t.ok(lockfile.packages![id].dependencies!['dep-of-pkg-with-1-dep'], `has dependency resolved for ${id}`)
    t.ok(lockfile.packages![id].resolution, `has resolution for ${id}`)
    t.ok(lockfile.packages![id].resolution['integrity'], `has integrity for package in the default registry`) // tslint:disable-line
    t.notOk(lockfile.packages![id].resolution['tarball'], `has no tarball for package in the default registry`) // tslint:disable-line

    const absDepPath = 'github.com/kevva/is-negative/1d7e288222b53a0cab90a331f1865220ec29560c'
    t.ok(lockfile.packages![absDepPath])
    t.ok(lockfile.packages![absDepPath].name, 'github-hosted package has name specified')
  }
})

test(`doing named installation when shared ${WANTED_LOCKFILE} exists already`, async (t: tape.Test) => {
  const pkg1 = {
    name: 'pkg1',
    version: '1.0.0',

    dependencies: {
      'is-negative': '^2.1.0',
    },
  }
  let pkg2: PackageJson = {
    name: 'pkg2',
    version: '1.0.0',

    dependencies: {
      'is-positive': '^3.1.0',
    },
  }
  const projects = preparePackages(t, [
    pkg1,
    pkg2,
  ])

  await writeYamlFile(WANTED_LOCKFILE, {
    importers: {
      pkg1: {
        dependencies: {
          'is-negative': '2.1.0',
        },
        specifiers: {
          'is-negative': '^2.1.0',
        },
      },
      pkg2: {
        dependencies: {
          'is-positive': '3.1.0',
        },
        specifiers: {
          'is-positive': '^3.1.0',
        },
      },
    },
    lockfileVersion: 5,
    packages: {
      '/is-negative/2.1.0': {
        resolution: {
          tarball: 'http://localhost:4873/is-negative/-/is-negative-2.1.0.tgz',
        },
      },
      '/is-positive/3.1.0': {
        resolution: {
          integrity: 'sha1-hX21hKG6XRyymAUn/DtsQ103sP0=',
        },
      },
    },
  })

  pkg2 = await addDependenciesToPackage(
    pkg2,
    ['is-positive'],
    await testDefaults({
      lockfileDirectory: process.cwd(),
      prefix: path.resolve('pkg2'),
    }),
  )

  const currentLockfile = await readYamlFile<Lockfile>(path.resolve(CURRENT_LOCKFILE))

  t.deepEqual(R.keys(currentLockfile['importers']), ['pkg2'], 'only pkg2 added to importers of current lockfile')

  await mutateModules(
    [
      {
        buildIndex: 0,
        manifest: pkg1,
        mutation: 'install',
        prefix: path.resolve('pkg1'),
      },
      {
        buildIndex: 0,
        manifest: pkg2,
        mutation: 'install',
        prefix: path.resolve('pkg2'),
      },
    ],
    await testDefaults(),
  )

  await projects['pkg1'].has('is-negative')
  await projects['pkg2'].has('is-positive')
})

// Covers https://github.com/pnpm/pnpm/issues/1200
test(`use current ${WANTED_LOCKFILE} as initial wanted one, when wanted was removed`, async (t) => {
  const project = prepareEmpty(t)

  const manifest = await addDependenciesToPackage({}, ['lodash@4.17.11', 'underscore@1.9.0'], await testDefaults())

  await rimraf(WANTED_LOCKFILE)

  await addDependenciesToPackage(manifest, ['underscore@1.9.1'], await testDefaults())

  await project.has('lodash')
  await project.has('underscore')
})
