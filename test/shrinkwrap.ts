import path = require('path')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import writeYamlFile = require('write-yaml-file')
import exists = require('path-exists')
import {prepare, testDefaults, addDistTag} from './utils'
import {installPkgs, install} from '../src'
import loadJsonFile = require('load-json-file')
import writePkg = require('write-pkg')
import rimraf = require('rimraf-then')
import sinon = require('sinon')

const test = promisifyTape(tape)

test('shrinkwrap file has correct format', async t => {
  const project = prepare(t)

  await installPkgs(['pkg-with-1-dep', '@rstacruz/tap-spec@4.1.1', 'kevva/is-negative'], testDefaults({save: true}))

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
})

test('shrinkwrap file has dev deps even when installing for prod only', async (t: tape.Test) => {
  const project = prepare(t, {
    devDependencies: {
      'is-negative': '2.1.0',
    },
  })

  await install(testDefaults({production: true}))

  const shr = await project.loadShrinkwrap()
  const id = '/is-negative/2.1.0'

  t.ok(shr.devDependencies, 'has devDependencies field')

  t.equal(shr.devDependencies['is-negative'], '2.1.0', 'has dev dependency resolved')

  t.ok(shr.packages[id], `has resolution for ${id}`)
})

test('shrinkwrap with scoped package', async t => {
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
    version: 3,
  })

  await install(testDefaults())
})

test('fail when shasum from shrinkwrap does not match with the actual one', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      'is-negative': '2.1.0',
    },
  })

  await writeYamlFile('shrinkwrap.yaml', {
    version: 3,
    registry: 'http://localhost:4873',
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
  })

  try {
    await install(testDefaults())
    t.fail('installation should have failed')
  } catch (err) {
    t.equal(err.code, 'EINTEGRITY')
  }
})

test("shrinkwrap doesn't lock subdependencies that don't satisfy the new specs", async (t: tape.Test) => {
  const project = prepare(t)

  // dependends on react-onclickoutside@5.9.0
  await installPkgs(['react-datetime@2.8.8'], testDefaults({save: true}))

  // dependends on react-onclickoutside@0.3.4
  await installPkgs(['react-datetime@1.3.0'], testDefaults({save: true}))

  t.equal(
    project.requireModule('.localhost+4873/react-datetime/1.3.0/node_modules/react-onclickoutside/package.json').version,
    '0.3.4',
    'react-datetime@1.3.0 has react-onclickoutside@0.3.4 in its node_modules')

  const shr = await project.loadShrinkwrap()

  t.equal(Object.keys(shr.dependencies).length, 1, 'resolutions not duplicated')
})

test('shrinkwrap not created when no deps in package.json', async (t: tape.Test) => {
  const project = prepare(t)

  await install(testDefaults())

  t.notOk(await project.loadShrinkwrap(), 'shrinkwrap file not created')
  t.notOk(await exists('node_modules'), 'empty node_modules not created')
})

test('shrinkwrap removed when no deps in package.json', async t => {
  const project = prepare(t)

  await writeYamlFile('shrinkwrap.yaml', {
    version: 3,
    registry: 'http://localhost:4873',
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
  })

  await install(testDefaults())

  t.notOk(await project.loadShrinkwrap(), 'shrinkwrap file removed')
})

test('shrinkwrap is fixed when it does not match package.json', async (t: tape.Test) => {
  const project = prepare(t, {
    devDependencies: {
      'is-negative': '^2.1.0',
    },
    optionalDependencies: {
      'is-positive': '^3.1.0'
    }
  })

  await writeYamlFile('shrinkwrap.yaml', {
    version: 3,
    registry: 'http://localhost:4873',
    dependencies: {
      'is-negative': '2.1.0',
      'is-positive': '3.1.0',
      '@types/semver': '5.3.31',
    },
    packages: {
      '/is-negative/2.1.0': {
        resolution: {
          tarball: 'http://localhost:4873/is-negative/-/is-negative-2.1.0.tgz',
        },
      },
      '/is-positive/3.1.0': {
        resolution: {
          integrity: 'sha1-hX21hKG6XRyymAUn/DtsQ103sP0='
        },
      },
      '/@types/semver/5.3.31': {
        resolution: {
          integrity: 'sha1-uZnX2TX0P1IHsBsA094ghS9Mp18=',
        },
      },
    },
    specifiers: {
      'is-negative': '^2.1.0',
      'is-positive': '^3.1.0',
      '@types/semver': '5.3.31',
    }
  })

  const reporter = sinon.spy()
  await install(testDefaults({reporter}))

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

test('respects shrinkwrap.yaml for top dependencies', async (t: tape.Test) => {
  const project = prepare(t)
  const reporter = sinon.spy()
  const fooProgress = sinon.match({
    name: 'pnpm:progress',
    status: 'resolving',
    pkg: {
      name: 'foo',
    },
  })

  const pkgs = ['foo', 'bar', 'qar']
  await Promise.all(pkgs.map(pkgName => addDistTag(pkgName, '100.0.0', 'latest')))

  await installPkgs(['foo'], testDefaults({save: true, reporter}))
  t.equal(reporter.withArgs(fooProgress).callCount, 1, 'reported foo once')
  await installPkgs(['bar'], testDefaults({saveOptional: true}))
  await installPkgs(['qar'], testDefaults({saveDev: true}))
  await installPkgs(['foobar'], testDefaults({save: true}))

  t.equal((await loadJsonFile(path.resolve('node_modules', 'foo', 'package.json'))).version, '100.0.0')
  t.equal((await loadJsonFile(path.resolve('node_modules', 'bar', 'package.json'))).version, '100.0.0')
  t.equal((await loadJsonFile(path.resolve('node_modules', 'qar', 'package.json'))).version, '100.0.0')
  t.equal((await loadJsonFile(path.resolve('node_modules', '.localhost+4873', 'foobar', '100.0.0', 'node_modules', 'foo', 'package.json'))).version, '100.0.0')
  t.equal((await loadJsonFile(path.resolve('node_modules', '.localhost+4873', 'foobar', '100.0.0', 'node_modules', 'bar', 'package.json'))).version, '100.0.0')

  await Promise.all(pkgs.map(pkgName => addDistTag(pkgName, '100.1.0', 'latest')))

  await rimraf('node_modules')
  await rimraf(path.join('..', '.store'))

  reporter.reset()

  // shouldn't care about what the registry in npmrc is
  // the one in shrinkwrap should be used
  await install(testDefaults({
    registry: 'https://registry.npmjs.org',
    rawNpmConfig: {
      registry: 'https://registry.npmjs.org',
    },
    reporter,
  }))

  t.equal(reporter.withArgs(fooProgress).callCount, 0, 'not reported foo')

  await project.storeHasNot('foo', '100.1.0')
  t.equal((await loadJsonFile(path.resolve('node_modules', 'foo', 'package.json'))).version, '100.0.0')
  t.equal((await loadJsonFile(path.resolve('node_modules', 'bar', 'package.json'))).version, '100.0.0')
  t.equal((await loadJsonFile(path.resolve('node_modules', 'qar', 'package.json'))).version, '100.0.0')
  t.equal((await loadJsonFile(path.resolve('node_modules', '.localhost+4873', 'foobar', '100.0.0', 'node_modules', 'foo', 'package.json'))).version, '100.0.0')
  t.equal((await loadJsonFile(path.resolve('node_modules', '.localhost+4873', 'foobar', '100.0.0', 'node_modules', 'bar', 'package.json'))).version, '100.0.0')
})

test('subdeps are updated on repeat install if outer shrinkwrap.yaml does not match the inner one', async (t: tape.Test) => {
  const project = prepare(t)

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'latest')

  await installPkgs(['pkg-with-1-dep'], testDefaults())

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  const shr = await project.loadShrinkwrap()

  t.ok(shr.packages['/dep-of-pkg-with-1-dep/100.0.0'])

  delete shr.packages['/dep-of-pkg-with-1-dep/100.0.0']

  shr.packages['/dep-of-pkg-with-1-dep/100.1.0'] = {
    resolution: {
      integrity: 'sha1-sdzLq5q5h7h61HeCB+HLf+lI+zw=',
    },
  }

  shr.packages['/pkg-with-1-dep/100.0.0']['dependencies']['dep-of-pkg-with-1-dep'] = '100.1.0'

  await writeYamlFile('shrinkwrap.yaml', shr)

  await install(testDefaults())

  await project.storeHas('dep-of-pkg-with-1-dep', '100.1.0')
})

test("recreates shrinkwrap file if it doesn't match the dependencies in package.json", async (t: tape.Test) => {
  const project = prepare(t)

  await installPkgs(['is-negative@1.0.0'], testDefaults({saveExact: true, saveProd: true}))
  await installPkgs(['is-positive@1.0.0'], testDefaults({saveExact: true, saveDev: true}))
  await installPkgs(['map-obj@1.0.0'], testDefaults({saveExact: true, saveOptional: true}))

  const shr1 = await project.loadShrinkwrap()
  t.equal(shr1.dependencies['is-negative'], '1.0.0')
  t.equal(shr1.specifiers['is-negative'], '1.0.0')

  const pkg = await loadJsonFile('package.json')

  pkg.dependencies['is-negative'] = '^2.1.0'
  pkg.devDependencies['is-positive'] = '^2.0.0'
  pkg.optionalDependencies['map-obj'] = '1.0.1'

  await writePkg(pkg)

  await install(testDefaults())

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

  await installPkgs(['highmaps-release@5.0.11'], testDefaults())

  const shr1 = await project.loadShrinkwrap()

  t.equal(shr1.dependencies['highmaps-release'], '5.0.11', 'dependency added correctly to shrinkwrap.yaml')

  await rimraf('node_modules')

  await install(testDefaults())

  const shr2 = await project.loadShrinkwrap()

  t.deepEqual(shr1, shr2, "shrinkwrap file hasn't been changed")
})

test('package is not marked dev if it is also a subdep of a regular dependency', async (t: tape.Test) => {
  const project = prepare(t)

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'latest')

  await installPkgs(['pkg-with-1-dep'], testDefaults())

  t.pass('installed pkg-with-1-dep')

  await installPkgs(['dep-of-pkg-with-1-dep'], testDefaults({saveDev: true}))

  t.pass('installed optional dependency which is also a dependency of pkg-with-1-dep')

  const shr = await project.loadShrinkwrap()

  t.notOk(shr.packages['/dep-of-pkg-with-1-dep/100.0.0']['dev'], 'package is not marked as dev')
})

test('package is not marked optional if it is also a subdep of a regular dependency', async (t: tape.Test) => {
  const project = prepare(t)

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'latest')

  await installPkgs(['pkg-with-1-dep'], testDefaults())
  await installPkgs(['dep-of-pkg-with-1-dep'], testDefaults({saveOptional: true}))

  const shr = await project.loadShrinkwrap()

  t.notOk(shr.packages['/dep-of-pkg-with-1-dep/100.0.0']['optional'], 'package is not marked as optional')
})

test('scoped module from different registry', async function (t: tape.Test) {
  const project = prepare(t)
  await installPkgs(['@zkochan/foo', 'is-positive'], testDefaults({
    rawNpmConfig: {
      '@zkochan:registry': 'https://registry.npmjs.org/'
    }
  }))

  const m = project.requireModule('@zkochan/foo')
  t.ok(m, 'foo is available')

  const shr = await project.loadShrinkwrap()

  t.deepEqual(shr, {
    dependencies: {
      '@zkochan/foo': 'registry.npmjs.org/@zkochan/foo/1.0.0',
      'is-positive': '3.1.0'
    },
    packages: {
      '/is-positive/3.1.0': {
        resolution: {
          integrity: 'sha1-hX21hKG6XRyymAUn/DtsQ103sP0='
        }
      },
      'registry.npmjs.org/@zkochan/foo/1.0.0': {
        resolution: {
          integrity: 'sha512-IFvrYpq7E6BqKex7A7czIFnFncPiUVdhSzGhAOWpp8RlkXns4y/9ZdynxaA/e0VkihRxQkihE2pTyvxjfe/wBg==',
          registry: 'https://registry.npmjs.org/',
          tarball: 'https://registry.npmjs.org/@zkochan/foo/-/foo-1.0.0.tgz'
        }
      }
    },
    registry: 'http://localhost:4873/',
    specifiers: {
      '@zkochan/foo': '^1.0.0',
      'is-positive': '^3.1.0',
    },
    shrinkwrapVersion: 3
  })
})

test('repeat install with no inner shrinkwrap should not rewrite packages in node_modules', async (t: tape.Test) => {
  const project = prepare(t)

  await installPkgs(['is-negative@1.0.0'], testDefaults())

  await rimraf('node_modules/.shrinkwrap.yaml')

  await install(testDefaults())

  const m = project.requireModule('is-negative')
  t.ok(m)
})

test('installing from shrinkwrap when using npm enterprise', async (t: tape.Test) => {
  const project = prepare(t)

  const opts = testDefaults({registry: 'https://npm-registry.compass.com/'})

  await installPkgs(['is-positive@3.1.0'], opts)

  const shr = await project.loadShrinkwrap()

  t.deepEqual(shr, {
    dependencies: {
      'is-positive': '3.1.0'
    },
    packages: {
      '/is-positive/3.1.0': {
        resolution: {
          integrity: 'sha1-hX21hKG6XRyymAUn/DtsQ103sP0=',
          tarball: '/i/is-positive/_attachments/is-positive-3.1.0.tgz'
        }
      },
    },
    registry: 'https://npm-registry.compass.com/',
    specifiers: {
      'is-positive': '^3.1.0',
    },
    shrinkwrapVersion: 3
  })

  await rimraf(opts.store)
  await rimraf('node_modules')

  await install(opts)

  project.has('is-positive')
})
