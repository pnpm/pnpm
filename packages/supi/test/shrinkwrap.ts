import { RootLog } from '@pnpm/core-loggers'
import prepare, { preparePackages } from '@pnpm/prepare'
import { fromDir as readPackageJsonFromDir } from '@pnpm/read-package-json'
import { Shrinkwrap } from '@pnpm/shrinkwrap-file'
import { stripIndent } from 'common-tags'
import mkdir = require('mkdirp-promise')
import fs = require('mz/fs')
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
import writePkg = require('write-pkg')
import writeYamlFile = require('write-yaml-file')
import {
  addDistTag,
  testDefaults,
} from './utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)
test['skip'] = promisifyTape(tape.skip) // tslint:disable-line:no-string-literal

const SHRINKWRAP_WARN_LOG = {
  level: 'warn',
  message: 'A shrinkwrap.yaml file exists. The current configuration prohibits to read or write a shrinkwrap file',
  name: 'pnpm',
}

test('shrinkwrap file has correct format', async (t: tape.Test) => {
  const project = prepare(t)

  await addDependenciesToPackage(
    [
      'pkg-with-1-dep',
      '@rstacruz/tap-spec@4.1.1',
      'kevva/is-negative#1d7e288222b53a0cab90a331f1865220ec29560c',
    ], await testDefaults({ save: true }))

  const modules = await project.loadModules()
  t.ok(modules)
  t.equal(modules!.pendingBuilds.length, 0)

  const shr = await project.loadShrinkwrap()
  const id = '/pkg-with-1-dep/100.0.0'

  t.equal(shr.shrinkwrapVersion, 3, 'correct shrinkwrap version')

  t.ok(shr.registry, 'has registry field')

  t.ok(shr.specifiers, 'has specifiers field')
  t.ok(shr.dependencies, 'has dependencies field')
  t.equal(shr.dependencies['pkg-with-1-dep'], '100.0.0', 'has dependency resolved')
  t.ok(shr.dependencies['@rstacruz/tap-spec'], 'has scoped dependency resolved')
  t.ok(shr.dependencies['is-negative'].indexOf('/') !== -1, 'has not shortened tarball from the non-standard registry')

  t.ok(shr.packages, 'has packages field')
  t.ok(shr.packages[id], `has resolution for ${id}`)
  t.ok(shr.packages[id].dependencies, `has dependency resolutions for ${id}`)
  t.ok(shr.packages[id].dependencies['dep-of-pkg-with-1-dep'], `has dependency resolved for ${id}`)
  t.ok(shr.packages[id].resolution, `has resolution for ${id}`)
  t.ok(shr.packages[id].resolution.integrity, `has integrity for package in the default registry`)
  t.notOk(shr.packages[id].resolution.tarball, `has no tarball for package in the default registry`)

  const absDepPath = 'github.com/kevva/is-negative/1d7e288222b53a0cab90a331f1865220ec29560c'
  t.ok(shr.packages[absDepPath])
  t.ok(shr.packages[absDepPath].name, 'github-hosted package has name specified')
})

test('shrinkwrap file has dev deps even when installing for prod only', async (t: tape.Test) => {
  const project = prepare(t, {
    devDependencies: {
      'is-negative': '2.1.0',
    },
  })

  await install(await testDefaults({ production: true }))

  const shr = await project.loadShrinkwrap()
  const id = '/is-negative/2.1.0'

  t.ok(shr.devDependencies, 'has devDependencies field')

  t.equal(shr.devDependencies['is-negative'], '2.1.0', 'has dev dependency resolved')

  t.ok(shr.packages[id], `has resolution for ${id}`)
})

test('shrinkwrap with scoped package', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      '@types/semver': '^5.3.31',
    },
  })

  await writeYamlFile('shrinkwrap.yaml', {
    dependencies: {
      '@types/semver': '5.3.31',
    },
    packages: {
      '/@types/semver/5.3.31': {
        resolution: {
          integrity: 'sha1-uZnX2TX0P1IHsBsA094ghS9Mp18=',
        },
      },
    },
    registry: 'http://localhost:4873',
    shrinkwrapVersion: 3,
    specifiers: {
      '@types/semver': '^5.3.31',
    },
  })

  await install(await testDefaults({ frozenShrinkwrap: true }))
})

test('fail when shasum from shrinkwrap does not match with the actual one', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      'is-negative': '2.1.0',
    },
  })

  await writeYamlFile('shrinkwrap.yaml', {
    dependencies: {
      'is-negative': '2.1.0',
    },
    packages: {
      '/is-negative/2.1.0': {
        resolution: {
          integrity: 'sha1-uZnX2TX0P1IHsBsA094ghS9Mp10=',
          tarball: 'http://localhost:4873/is-negative/-/is-negative-2.1.0.tgz',
        },
      },
    },
    registry: 'http://localhost:4873',
    shrinkwrapVersion: 3,
    specifiers: {
      'is-negative': '2.1.0',
    },
  })

  try {
    await install(await testDefaults())
    t.fail('installation should have failed')
  } catch (err) {
    t.equal(err.code, 'EINTEGRITY')
  }
})

test("shrinkwrap doesn't lock subdependencies that don't satisfy the new specs", async (t: tape.Test) => {
  const project = prepare(t)

  // dependends on react-onclickoutside@5.9.0
  await addDependenciesToPackage(['react-datetime@2.8.8'], await testDefaults({ save: true }))

  // dependends on react-onclickoutside@0.3.4
  await addDependenciesToPackage(['react-datetime@1.3.0'], await testDefaults({ save: true }))

  t.equal(
    project.requireModule('.localhost+4873/react-datetime/1.3.0/node_modules/react-onclickoutside/package.json').version,
    '0.3.4',
    'react-datetime@1.3.0 has react-onclickoutside@0.3.4 in its node_modules')

  const shr = await project.loadShrinkwrap()

  t.equal(Object.keys(shr.dependencies).length, 1, 'resolutions not duplicated')
})

test('shrinkwrap not created when no deps in package.json', async (t: tape.Test) => {
  const project = prepare(t)

  await install(await testDefaults())

  t.notOk(await project.loadShrinkwrap(), 'shrinkwrap file not created')
  t.notOk(await exists('node_modules'), 'empty node_modules not created')
})

test('shrinkwrap removed when no deps in package.json', async (t: tape.Test) => {
  const project = prepare(t)

  await writeYamlFile('shrinkwrap.yaml', {
    dependencies: {
      'is-negative': '2.1.0',
    },
    packages: {
      '/is-negative/2.1.0': {
        resolution: {
          tarball: 'http://localhost:4873/is-negative/-/is-negative-2.1.0.tgz',
        },
      },
    },
    registry: 'http://localhost:4873',
    shrinkwrapVersion: 3,
    specifiers: {
      'is-negative': '2.1.0',
    },
  })

  await install(await testDefaults())

  t.notOk(await project.loadShrinkwrap(), 'shrinkwrap file removed')
})

test('shrinkwrap is fixed when it does not match package.json', async (t: tape.Test) => {
  const project = prepare(t, {
    devDependencies: {
      'is-negative': '^2.1.0',
    },
    optionalDependencies: {
      'is-positive': '^3.1.0',
    },
  })

  await writeYamlFile('shrinkwrap.yaml', {
    dependencies: {
      '@types/semver': '5.3.31',
      'is-negative': '2.1.0',
      'is-positive': '3.1.0',
    },
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
    registry: 'http://localhost:4873',
    shrinkwrapVersion: 3,
    specifiers: {
      '@types/semver': '5.3.31',
      'is-negative': '^2.1.0',
      'is-positive': '^3.1.0',
    },
  })

  const reporter = sinon.spy()
  await install(await testDefaults({ reporter }))

  const progress = sinon.match({
    name: 'pnpm:progress',
    status: 'resolving',
  })
  t.equal(reporter.withArgs(progress).callCount, 0, 'resolving not reported')

  const shr = await project.loadShrinkwrap()

  t.equal(shr.devDependencies['is-negative'], '2.1.0', 'is-negative moved to devDependencies in shrinkwrap.yaml')
  t.equal(shr.optionalDependencies['is-positive'], '3.1.0', 'is-positive moved to optionalDependencies in shrinkwrap.yaml')
  t.notOk(shr.dependencies, 'empty dependencies property removed')
  t.notOk(shr.packages['/@types/semver/5.3.31'], 'package not referenced in package.json removed')
})

test('doing named installation when shrinkwrap.yaml exists already', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      '@types/semver': '5.3.31',
      'is-negative': '^2.1.0',
      'is-positive': '^3.1.0',
    },
  })

  await writeYamlFile('shrinkwrap.yaml', {
    dependencies: {
      '@types/semver': '5.3.31',
      'is-negative': '2.1.0',
      'is-positive': '3.1.0',
    },
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
    registry: 'http://localhost:4873',
    shrinkwrapVersion: 3,
    specifiers: {
      '@types/semver': '5.3.31',
      'is-negative': '^2.1.0',
      'is-positive': '^3.1.0',
    },
  })

  const reporter = sinon.spy()

  await addDependenciesToPackage(['is-positive'], await testDefaults({ reporter }))
  await install(await testDefaults({ reporter }))

  t.notOk(reporter.calledWithMatch(SHRINKWRAP_WARN_LOG), 'no warning about ignoring shrinkwrap.yaml')

  await project.has('is-negative')
})

test('respects shrinkwrap.yaml for top dependencies', async (t: tape.Test) => {
  const project = prepare(t)
  const reporter = sinon.spy()
  // const fooProgress = sinon.match({
  //   name: 'pnpm:progress',
  //   status: 'resolving',
  //   pkg: {
  //     name: 'foo',
  //   },
  // })

  const pkgs = ['foo', 'bar', 'qar']
  await Promise.all(pkgs.map((pkgName) => addDistTag(pkgName, '100.0.0', 'latest')))

  await addDependenciesToPackage(['foo'], await testDefaults({ save: true, reporter }))
  // t.equal(reporter.withArgs(fooProgress).callCount, 1, 'reported foo once')
  await addDependenciesToPackage(['bar'], await testDefaults({ targetDependenciesField: 'optionalDependencies' }))
  await addDependenciesToPackage(['qar'], await testDefaults({ addDependenciesToPackage: 'devDependencies' }))
  await addDependenciesToPackage(['foobar'], await testDefaults({ save: true }))

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
  // the one in shrinkwrap should be used
  await install(await testDefaults({
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

test('subdeps are updated on repeat install if outer shrinkwrap.yaml does not match the inner one', async (t: tape.Test) => {
  const project = prepare(t)

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'latest')

  await addDependenciesToPackage(['pkg-with-1-dep'], await testDefaults())

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  const shr = await project.loadShrinkwrap()

  t.ok(shr.packages['/dep-of-pkg-with-1-dep/100.0.0'])

  delete shr.packages['/dep-of-pkg-with-1-dep/100.0.0']

  shr.packages['/dep-of-pkg-with-1-dep/100.1.0'] = {
    resolution: {
      integrity: getIntegrity('dep-of-pkg-with-1-dep', '100.1.0'),
    },
  }

  shr.packages['/pkg-with-1-dep/100.0.0'].dependencies['dep-of-pkg-with-1-dep'] = '100.1.0'

  await writeYamlFile('shrinkwrap.yaml', shr)

  await install(await testDefaults())

  await project.storeHas('dep-of-pkg-with-1-dep', '100.1.0')
})

test("recreates shrinkwrap file if it doesn't match the dependencies in package.json", async (t: tape.Test) => {
  const project = prepare(t)

  await addDependenciesToPackage(['is-negative@1.0.0'], await testDefaults({ pinnedVersion: 'patch', targetDependenciesField: 'dependencies' }))
  await addDependenciesToPackage(['is-positive@1.0.0'], await testDefaults({ pinnedVersion: 'patch', targetDependenciesField: 'devDependencies' }))
  await addDependenciesToPackage(['map-obj@1.0.0'], await testDefaults({ pinnedVersion: 'patch', targetDependenciesField: 'optionalDependencies' }))

  const shr1 = await project.loadShrinkwrap()
  t.equal(shr1.dependencies['is-negative'], '1.0.0')
  t.equal(shr1.specifiers['is-negative'], '1.0.0')

  const pkg = await readPackageJsonFromDir(process.cwd())

  pkg.dependencies!['is-negative'] = '^2.1.0'
  pkg.devDependencies!['is-positive'] = '^2.0.0'
  pkg.optionalDependencies!['map-obj'] = '1.0.1'

  await writePkg(pkg)

  await install(await testDefaults())

  const shr = await project.loadShrinkwrap()

  t.equal(shr.dependencies['is-negative'], '2.1.0')
  t.equal(shr.specifiers['is-negative'], '^2.1.0')

  t.equal(shr.devDependencies['is-positive'], '2.0.0')
  t.equal(shr.specifiers['is-positive'], '^2.0.0')

  t.equal(shr.optionalDependencies['map-obj'], '1.0.1')
  t.equal(shr.specifiers['map-obj'], '1.0.1')
})

test('repeat install with shrinkwrap should not mutate shrinkwrap when dependency has version specified with v prefix', async (t: tape.Test) => {
  const project = prepare(t)

  await addDependenciesToPackage(['highmaps-release@5.0.11'], await testDefaults())

  const shr1 = await project.loadShrinkwrap()

  t.equal(shr1.dependencies['highmaps-release'], '5.0.11', 'dependency added correctly to shrinkwrap.yaml')

  await rimraf('node_modules')

  await install(await testDefaults())

  const shr2 = await project.loadShrinkwrap()

  t.deepEqual(shr1, shr2, "shrinkwrap file hasn't been changed")
})

test('package is not marked dev if it is also a subdep of a regular dependency', async (t: tape.Test) => {
  const project = prepare(t)

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'latest')

  await addDependenciesToPackage(['pkg-with-1-dep'], await testDefaults())

  t.pass('installed pkg-with-1-dep')

  await addDependenciesToPackage(['dep-of-pkg-with-1-dep'], await testDefaults({ targetDependenciesField: 'devDependencies' }))

  t.pass('installed optional dependency which is also a dependency of pkg-with-1-dep')

  const shr = await project.loadShrinkwrap()

  t.notOk(shr.packages['/dep-of-pkg-with-1-dep/100.0.0'].dev, 'package is not marked as dev')
})

test('package is not marked optional if it is also a subdep of a regular dependency', async (t: tape.Test) => {
  const project = prepare(t)

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'latest')

  await addDependenciesToPackage(['pkg-with-1-dep'], await testDefaults())
  await addDependenciesToPackage(['dep-of-pkg-with-1-dep'], await testDefaults({ targetDependenciesField: 'optionalDependencies' }))

  const shr = await project.loadShrinkwrap()

  t.notOk(shr.packages['/dep-of-pkg-with-1-dep/100.0.0'].optional, 'package is not marked as optional')
})

test('scoped module from different registry', async (t: tape.Test) => {
  const project = prepare(t)

  const opts = await testDefaults()
  opts.registries!['@zkochan'] = 'https://registry.npmjs.org/' // tslint:disable-line
  await addDependenciesToPackage(['@zkochan/foo', 'is-positive'], opts)

  const m = project.requireModule('@zkochan/foo')
  t.ok(m, 'foo is available')

  const shr = await project.loadShrinkwrap()

  t.deepEqual(shr, {
    dependencies: {
      '@zkochan/foo': 'registry.npmjs.org/@zkochan/foo/1.0.0',
      'is-positive': '3.1.0',
    },
    packages: {
      '/is-positive/3.1.0': {
        dev: false,
        engines: {
          node: '>=0.10.0',
        },
        resolution: {
          integrity: 'sha1-hX21hKG6XRyymAUn/DtsQ103sP0=',
        },
      },
      'registry.npmjs.org/@zkochan/foo/1.0.0': {
        dev: false,
        name: '@zkochan/foo',
        resolution: {
          integrity: 'sha512-IFvrYpq7E6BqKex7A7czIFnFncPiUVdhSzGhAOWpp8RlkXns4y/9ZdynxaA/e0VkihRxQkihE2pTyvxjfe/wBg==',
          registry: 'https://registry.npmjs.org/',
          tarball: 'https://registry.npmjs.org/@zkochan/foo/-/foo-1.0.0.tgz',
        },
        version: '1.0.0',
      },
    },
    registry: 'http://localhost:4873/',
    shrinkwrapMinorVersion: 9,
    shrinkwrapVersion: 3,
    specifiers: {
      '@zkochan/foo': '^1.0.0',
      'is-positive': '^3.1.0',
    },
  })
})

test('repeat install with no inner shrinkwrap should not rewrite packages in node_modules', async (t: tape.Test) => {
  const project = prepare(t)

  await addDependenciesToPackage(['is-negative@1.0.0'], await testDefaults())

  await rimraf('node_modules/.shrinkwrap.yaml')

  await install(await testDefaults())

  const m = project.requireModule('is-negative')
  t.ok(m)
})

// Skipped because the npm-registry.compass.com server was down
// might be a good idea to mock it
// tslint:disable-next-line:no-string-literal
test['skip']('installing from shrinkwrap when using npm enterprise', async (t: tape.Test) => {
  const project = prepare(t)

  const opts = await testDefaults({ registry: 'https://npm-registry.compass.com/' })

  await addDependenciesToPackage(['is-positive@3.1.0'], opts)

  const shr = await project.loadShrinkwrap()

  t.deepEqual(shr, {
    dependencies: {
      'is-positive': '3.1.0',
    },
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
    registry: 'https://npm-registry.compass.com/',
    shrinkwrapMinorVersion: 9,
    shrinkwrapVersion: 3,
    specifiers: {
      'is-positive': '^3.1.0',
    },
  })

  await rimraf(opts.store)
  await rimraf('node_modules')

  await install(opts)

  await project.has('is-positive')
})

test('packages are placed in devDependencies even if they are present as non-dev as well', async (t: tape.Test) => {
  const project = prepare(t, {
    devDependencies: {
      'dep-of-pkg-with-1-dep': '^100.1.0',
      'pkg-with-1-dep': '^100.0.0',
    },
  })

  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  const reporter = sinon.spy()
  await install(await testDefaults({ reporter }))

  const shr = await project.loadShrinkwrap()

  t.ok(shr.devDependencies['dep-of-pkg-with-1-dep'])
  t.ok(shr.devDependencies['pkg-with-1-dep'])

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
  const project = prepare(t)

  await addDependenciesToPackage(['has-github-dep@1'], await testDefaults())
  await addDependenciesToPackage(['has-github-dep@latest'], await testDefaults())

  t.pass('installation of latest did not fail')
})

test('updating package that has deps with peers', async (t: tape.Test) => {
  const project = prepare(t)

  await addDependenciesToPackage(['abc-grand-parent-with-c@0'], await testDefaults())
  await addDependenciesToPackage(['abc-grand-parent-with-c@1'], await testDefaults())

  t.pass('installation of latest did not fail')
})

test('updating shrinkwrap version 3 to 3.5', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      'abc-grand-parent-with-c': '^1.0.0',
    },
  })

  const shrV3Content = stripIndent`
    dependencies:
      abc-grand-parent-with-c: 1.0.0
    packages:
      /abc-grand-parent-with-c/1.0.0:
        dependencies:
          abc-parent-with-ab: /abc-parent-with-ab/1.0.0/peer-c@1.0.0
          peer-c: 1.0.0
        resolution:
          integrity: ${getIntegrity('abc-grand-parent-with-c', '1.0.0')}
      /abc-parent-with-ab/1.0.0/peer-c@1.0.0:
        dependencies:
          abc: /abc/1.0.0/165e1e08a3f7e7f77ddb572ad0e55660
          peer-a: 1.0.0
          peer-b: 1.0.0
        id: localhost+4873/abc-parent-with-ab/1.0.0
        resolution:
          integrity: ${getIntegrity('abc-parent-with-ab', '1.0.0')}
      /abc/1.0.0/165e1e08a3f7e7f77ddb572ad0e55660:
        dependencies:
          dep-of-pkg-with-1-dep: 100.0.0
          peer-a: 1.0.0
          peer-b: 1.0.0
          peer-c: 1.0.0
        id: localhost+4873/abc/1.0.0
        resolution:
          integrity: ${getIntegrity('abc', '1.0.0')}
      /dep-of-pkg-with-1-dep/100.0.0:
        resolution:
          integrity: ${getIntegrity('dep-of-pkg-with-1-dep', '100.0.0')}
      /peer-a/1.0.0:
        resolution:
          integrity: ${getIntegrity('peer-a', '1.0.0')}
      /peer-b/1.0.0:
        resolution:
          integrity: ${getIntegrity('peer-b', '1.0.0')}
      /peer-c/1.0.0:
        resolution:
          integrity: ${getIntegrity('peer-c', '1.0.0')}
    registry: 'http://localhost:4873/'
    shrinkwrapVersion: 3
    specifiers:
      abc-grand-parent-with-c: ^1.0.0
  `

  await fs.writeFile('shrinkwrap.yaml', shrV3Content, 'utf8')

  await install(await testDefaults())

  const shr = await project.loadShrinkwrap()

  t.equal(shr.shrinkwrapMinorVersion, 9)
  t.ok(shr.packages['/abc/1.0.0/165e1e08a3f7e7f77ddb572ad0e55660'].peerDependencies)
})

test('pendingBuilds gets updated if install removes packages', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      'pre-and-postinstall-scripts-example': '*',
      'with-postinstall-b': '*',
    },
  })

  await install(await testDefaults({ ignoreScripts: true }))
  const modules1 = await project.loadModules()

  await project.writePackageJson({
    dependencies: {
      'pre-and-postinstall-scripts-example': '*',
    },
  })

  await install(await testDefaults({ ignoreScripts: true }))
  const modules2 = await project.loadModules()

  t.ok(modules1)
  t.ok(modules2)
  t.ok(modules1!.pendingBuilds.length > modules2!.pendingBuilds.length, 'pendingBuilds gets updated when install removes packages')
})

test('dev properties are correctly updated on named install', async (t: tape.Test) => {
  const project = prepare(t)

  await addDependenciesToPackage(['inflight@1.0.6'], await testDefaults({ targetDependenciesField: 'devDependencies' }))
  await addDependenciesToPackage(['foo@npm:inflight@1.0.6'], await testDefaults({}))

  const shr = await project.loadShrinkwrap()
  t.deepEqual(R.values(shr.packages).filter((dep) => typeof dep.dev !== 'undefined'), [], 'there are 0 packages with dev property in shrinkwrap.yaml')
})

test('optional properties are correctly updated on named install', async (t: tape.Test) => {
  const project = prepare(t)

  await addDependenciesToPackage(['inflight@1.0.6'], await testDefaults({ targetDependenciesField: 'optionalDependencies' }))
  await addDependenciesToPackage(['foo@npm:inflight@1.0.6'], await testDefaults({}))

  const shr = await project.loadShrinkwrap()
  t.deepEqual(R.values(shr.packages).filter((dep) => typeof dep.optional !== 'undefined'), [], 'there are 0 packages with optional property in shrinkwrap.yaml')
})

test('dev property is correctly set for package that is duplicated to both the dependencies and devDependencies group', async (t: tape.Test) => {
  const project = prepare(t)

  // TODO: use a smaller package for testing
  await addDependenciesToPackage(['overlap@2.2.8'], await testDefaults())

  const shr = await project.loadShrinkwrap()
  t.ok(shr.packages['/couleurs/5.0.0'].dev === false)
})

test('no shrinkwrap', async (t: tape.Test) => {
  const project = prepare(t)
  const reporter = sinon.spy()

  await addDependenciesToPackage(['is-positive'], await testDefaults({ shrinkwrap: false, reporter }))

  t.notOk(reporter.calledWithMatch(SHRINKWRAP_WARN_LOG), 'no warning about ignoring shrinkwrap.yaml')

  await project.has('is-positive')

  t.notOk(await project.loadShrinkwrap(), 'shrinkwrap.yaml not created')
})

test('shrinkwrap is ignored when shrinkwrap = false', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      'is-negative': '2.1.0',
    },
  })

  await writeYamlFile('shrinkwrap.yaml', {
    dependencies: {
      'is-negative': '2.1.0',
    },
    packages: {
      '/is-negative/2.1.0': {
        resolution: {
          integrity: 'sha1-uZnX2TX0P1IHsBsA094ghS9Mp10=', // Invalid integrity
          tarball: 'http://localhost:4873/is-negative/-/is-negative-2.1.0.tgz',
        },
      },
    },
    registry: 'http://localhost:4873',
    shrinkwrapVersion: 3,
    specifiers: {
      'is-negative': '2.1.0',
    },
  })

  const reporter = sinon.spy()

  await install(await testDefaults({ shrinkwrap: false, reporter }))

  t.ok(reporter.calledWithMatch(SHRINKWRAP_WARN_LOG), 'warning about ignoring shrinkwrap.yaml')

  await project.has('is-negative')

  t.ok(await project.loadShrinkwrap(), 'existing shrinkwrap.yaml not removed')
})

test("don't update shrinkwrap.yaml during uninstall when shrinkwrap: false", async (t: tape.Test) => {
  const project = prepare(t)

  {
    const reporter = sinon.spy()

    await addDependenciesToPackage(['is-positive'], await testDefaults({ reporter }))

    t.notOk(reporter.calledWithMatch(SHRINKWRAP_WARN_LOG), 'no warning about ignoring shrinkwrap.yaml')
  }

  {
    const reporter = sinon.spy()

    await uninstall(['is-positive'], await testDefaults({ shrinkwrap: false, reporter }))

    t.ok(reporter.calledWithMatch(SHRINKWRAP_WARN_LOG), 'warning about ignoring shrinkwrap.yaml')
  }

  await project.hasNot('is-positive')

  t.ok(await project.loadShrinkwrap(), 'shrinkwrap.yaml not removed during uninstall')
})

test('fail when installing with shrinkwrap: false and shrinkwrapOnly: true', async (t: tape.Test) => {
  const project = prepare(t)

  try {
    await install(await testDefaults({ shrinkwrap: false, shrinkwrapOnly: true }))
    t.fail('installation should have failed')
  } catch (err) {
    t.equal(err.message, 'Cannot generate a shrinkwrap.yaml because shrinkwrap is set to false')
  }
})

test("don't remove packages during named install when shrinkwrap: false", async (t: tape.Test) => {
  const project = prepare(t)

  await addDependenciesToPackage(['is-positive'], await testDefaults({ shrinkwrap: false }))
  await addDependenciesToPackage(['is-negative'], await testDefaults({ shrinkwrap: false }))

  await project.has('is-positive')
  await project.has('is-negative')
})

test('save tarball URL when it is non-standard', async (t: tape.Test) => {
  const project = prepare(t)

  await addDependenciesToPackage(['esprima-fb@3001.1.0-dev-harmony-fb'], await testDefaults())

  const shr = await project.loadShrinkwrap()

  t.equal(shr.packages['/esprima-fb/3001.1.0-dev-harmony-fb'].resolution.tarball, '/esprima-fb/-/esprima-fb-3001.0001.0000-dev-harmony-fb.tgz')
})

test('when package registry differs from default one, save it to resolution field', async (t: tape.Test) => {
  const project = prepare(t)

  await addDependenciesToPackage(['@zkochan/git-config', 'is-positive'], await testDefaults({
    rawNpmConfig: {
      '@zkochan:registry': 'https://registry.node-modules.io/',
      'registry': 'https://registry.npmjs.org/',
    },
    registries: {
      '@zkochan': 'https://registry.node-modules.io/',
      'default': 'https://registry.npmjs.org/',
    },
  }))

  const shr = await project.loadShrinkwrap()

  t.equal(shr.packages['/@zkochan/git-config/0.1.0'].resolution.registry, 'https://registry.node-modules.io/')
})

test('packages installed via tarball URL from the default registry are normalized', async (t: tape.Test) => {
  const project = prepare(t)

  await addDependenciesToPackage([
    'http://localhost:4873/pkg-with-tarball-dep-from-registry/-/pkg-with-tarball-dep-from-registry-1.0.0.tgz',
    'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
  ], await testDefaults())

  const shr = await project.loadShrinkwrap()

  t.deepEqual(shr, {
    dependencies: {
      'is-positive': 'registry.npmjs.org/is-positive/-/is-positive-1.0.0',
      'pkg-with-tarball-dep-from-registry': '1.0.0',
    },
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
    registry: 'http://localhost:4873/',
    shrinkwrapMinorVersion: 9,
    shrinkwrapVersion: 3,
    specifiers: {
      'is-positive': 'https://registry.npmjs.org/is-positive/-/is-positive-1.0.0.tgz',
      'pkg-with-tarball-dep-from-registry': 'http://localhost:4873/pkg-with-tarball-dep-from-registry/-/pkg-with-tarball-dep-from-registry-1.0.0.tgz',
    },
  })
})

test('shrinkwrap file has correct format when shrinkwrap directory does not equal the prefix directory', async (t: tape.Test) => {
  const project = prepare(t)

  await addDependenciesToPackage(['pkg-with-1-dep', '@rstacruz/tap-spec@4.1.1', 'kevva/is-negative#1d7e288222b53a0cab90a331f1865220ec29560c'],
    await testDefaults({ save: true, shrinkwrapDirectory: path.resolve('..') }))

  t.ok(!await exists('node_modules/.modules.yaml'), ".modules.yaml in importer's node_modules not created")

  process.chdir('..')

  const modules = await readYamlFile<object>(path.resolve('node_modules', '.modules.yaml'))
  t.ok(modules, '.modules.yaml in virtual store directory created')
  t.equal(modules['pendingBuilds'].length, 0) // tslint:disable-line:no-string-literal

  {
    const shr = await readYamlFile('shrinkwrap.yaml') as Shrinkwrap
    const id = '/pkg-with-1-dep/100.0.0'

    t.equal(shr.shrinkwrapVersion, 4, 'correct shrinkwrap version')

    t.notOk(shr.registry, 'has no registry field')

    t.ok(shr.importers)
    t.ok(shr.importers.project)
    t.ok(shr.importers.project.specifiers, 'has specifiers field')
    t.ok(shr.importers.project.dependencies, 'has dependencies field')
    t.equal(shr.importers.project.dependencies!['pkg-with-1-dep'], '100.0.0', 'has dependency resolved')
    t.ok(shr.importers.project.dependencies!['@rstacruz/tap-spec'], 'has scoped dependency resolved')
    t.ok(shr.importers.project.dependencies!['is-negative'].indexOf('/') !== -1, 'has not shortened tarball from the non-standard registry')

    t.ok(shr.packages, 'has packages field')
    t.ok(shr.packages![id], `has resolution for ${id}`)
    t.ok(shr.packages![id].dependencies, `has dependency resolutions for ${id}`)
    t.ok(shr.packages![id].dependencies!['dep-of-pkg-with-1-dep'], `has dependency resolved for ${id}`)
    t.ok(shr.packages![id].resolution, `has resolution for ${id}`)
    t.ok(shr.packages![id].resolution['integrity'], `has integrity for package in the default registry`) // tslint:disable-line
    t.notOk(shr.packages![id].resolution['tarball'], `has no tarball for package in the default registry`) // tslint:disable-line

    const absDepPath = 'github.com/kevva/is-negative/1d7e288222b53a0cab90a331f1865220ec29560c'
    t.ok(shr.packages![absDepPath])
    t.ok(shr.packages![absDepPath].name, 'github-hosted package has name specified')
  }

  await mkdir('project-2')

  process.chdir('project-2')

  await addDependenciesToPackage(['is-positive'], await testDefaults({ save: true, shrinkwrapDirectory: path.resolve('..') }))

  {
    const shr = await readYamlFile<Shrinkwrap>(path.join('..', 'shrinkwrap.yaml'))

    t.ok(shr.importers)
    t.ok(shr.importers['project-2'])

    // previous entries are not removed
    const id = '/pkg-with-1-dep/100.0.0'

    t.ok(shr.importers)
    t.ok(shr.importers.project)
    t.ok(shr.importers.project.specifiers, 'has specifiers field')
    t.ok(shr.importers.project.dependencies, 'has dependencies field')
    t.equal(shr.importers.project.dependencies!['pkg-with-1-dep'], '100.0.0', 'has dependency resolved')
    t.ok(shr.importers.project.dependencies!['@rstacruz/tap-spec'], 'has scoped dependency resolved')
    t.ok(shr.importers.project.dependencies!['is-negative'].indexOf('/') !== -1, 'has not shortened tarball from the non-standard registry')

    t.ok(shr.packages, 'has packages field')
    t.ok(shr.packages![id], `has resolution for ${id}`)
    t.ok(shr.packages![id].dependencies, `has dependency resolutions for ${id}`)
    t.ok(shr.packages![id].dependencies!['dep-of-pkg-with-1-dep'], `has dependency resolved for ${id}`)
    t.ok(shr.packages![id].resolution, `has resolution for ${id}`)
    t.ok(shr.packages![id].resolution['integrity'], `has integrity for package in the default registry`) // tslint:disable-line
    t.notOk(shr.packages![id].resolution['tarball'], `has no tarball for package in the default registry`) // tslint:disable-line

    const absDepPath = 'github.com/kevva/is-negative/1d7e288222b53a0cab90a331f1865220ec29560c'
    t.ok(shr.packages![absDepPath])
    t.ok(shr.packages![absDepPath].name, 'github-hosted package has name specified')
  }
})

test('doing named installation when shared shrinkwrap.yaml exists already', async (t: tape.Test) => {
  const projects = preparePackages(t, [
    {
      name: 'pkg1',
      version: '1.0.0',

      dependencies: {
        'is-negative': '^2.1.0',
      },
    },
    {
      name: 'pkg2',
      version: '1.0.0',

      dependencies: {
        'is-positive': '^3.1.0',
      },
    },
  ])

  await writeYamlFile('shrinkwrap.yaml', {
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
    shrinkwrapVersion: 4,
  })

  await addDependenciesToPackage(
    ['is-positive'],
    await testDefaults({
      prefix: path.resolve('pkg2'),
      shrinkwrapDirectory: process.cwd(),
    }),
  )

  const currentShr = await readYamlFile<Shrinkwrap>(path.resolve('node_modules', '.shrinkwrap.yaml'))

  t.deepEqual(R.keys(currentShr['importers']), ['pkg2'], 'only pkg2 added to importers of current shrinkwrap')

  await mutateModules(
    [
      {
        mutation: 'install',
        prefix: path.resolve('pkg1'),
      },
      {
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
test('use current shrinkwrap.yaml as initial wanted one, when wanted was removed', async (t) => {
  const project = prepare(t)

  await addDependenciesToPackage(['lodash@4.17.11', 'underscore@1.9.0'], await testDefaults())

  await rimraf('shrinkwrap.yaml')

  await addDependenciesToPackage(['underscore@1.9.1'], await testDefaults())

  await project.has('lodash')
  await project.has('underscore')
})
