import path = require('path')
import tape = require('tape')
import promisifyTape from 'tape-promise'
import writeYamlFile = require('write-yaml-file')
import exists = require('path-exists')
import {prepare, testDefaults, addDistTag} from './utils'
import {
  installPkgs,
  install,
  RootLog,
} from 'supi'
import loadJsonFile = require('load-json-file')
import writePkg = require('write-pkg')
import rimraf = require('rimraf-then')
import sinon = require('sinon')
import {stripIndent} from 'common-tags'
import fs = require('mz/fs')

const test = promisifyTape(tape)

test('shrinkwrap file has correct format', async (t: tape.Test) => {
  const project = prepare(t)

  await installPkgs(['pkg-with-1-dep', '@rstacruz/tap-spec@4.1.1', 'kevva/is-negative#1d7e288222b53a0cab90a331f1865220ec29560c'], testDefaults({save: true}))

  const modules = await project.loadModules()
  t.equal(modules['pendingBuilds'].length, 0)

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

test('doing named installation when shrinkwrap.yaml exists already', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      'is-negative': '^2.1.0',
      'is-positive': '^3.1.0',
      '@types/semver': '5.3.31',
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

  await installPkgs(['is-positive'], testDefaults())
  await install(testDefaults())

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
  await Promise.all(pkgs.map(pkgName => addDistTag(pkgName, '100.0.0', 'latest')))

  await installPkgs(['foo'], testDefaults({save: true, reporter}))
  // t.equal(reporter.withArgs(fooProgress).callCount, 1, 'reported foo once')
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

  // t.equal(reporter.withArgs(fooProgress).callCount, 0, 'not reported foo')

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
      integrity: 'sha1-TorYQB9bz9ktHFqtbcZrL1MxGK0=',
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
        dev: false,
        engines: {
          node: '>=0.10.0',
        },
        resolution: {
          integrity: 'sha1-hX21hKG6XRyymAUn/DtsQ103sP0='
        }
      },
      'registry.npmjs.org/@zkochan/foo/1.0.0': {
        name: '@zkochan/foo',
        dev: false,
        version: '1.0.0',
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
    shrinkwrapVersion: 3,
    shrinkwrapMinorVersion: 4,
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

// Skipped because the npm-registry.compass.com server was down
// might be a good idea to mock it
test['skip']('installing from shrinkwrap when using npm enterprise', async (t: tape.Test) => {
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
        dev: false,
        engines: {
          node: '>=0.10.0',
        },
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
    shrinkwrapVersion: 3,
    shrinkwrapMinorVersion: 4,
  })

  await rimraf(opts.store)
  await rimraf('node_modules')

  await install(opts)

  await project.has('is-positive')
})

test('packages are placed in devDependencies even if they are present as non-dev as well', async (t: tape.Test) => {
  const project = prepare(t, {
    devDependencies: {
      'pkg-with-1-dep': '^1.0.0',
      'dep-of-pkg-with-1-dep': '^1.1.0',
    },
  })

  await addDistTag('dep-of-pkg-with-1-dep', '1.1.0', 'latest')

  const reporter = sinon.spy()
  await install(testDefaults({reporter}))

  const shr = await project.loadShrinkwrap()

  t.ok(shr.devDependencies['dep-of-pkg-with-1-dep'])
  t.ok(shr.devDependencies['pkg-with-1-dep'])

  t.ok(reporter.calledWithMatch(<RootLog>{
    name: 'pnpm:root',
    level: 'info',
    added: {
      name: 'dep-of-pkg-with-1-dep',
      version: '1.1.0',
      dependencyType: 'dev',
    },
  }), 'dep-of-pkg-with-1-dep added to root')
  t.ok(reporter.calledWithMatch(<RootLog>{
    name: 'pnpm:root',
    level: 'info',
    added: {
      name: 'pkg-with-1-dep',
      version: '1.0.0',
      dependencyType: 'dev',
    },
  }), 'pkg-with-1-dep added to root')
})

// This testcase verifies that pnpm is not failing when trying to preserve dependencies.
// Only when a dependency is a range dependency, should pnpm try to compare versions of deps with semver.satisfies().
test('updating package that has a github-hosted dependency', async (t: tape.Test) => {
  const project = prepare(t)

  await installPkgs(['has-github-dep@1'], testDefaults())
  await installPkgs(['has-github-dep@latest'], testDefaults())

  t.pass('installation of latest did not fail')
})

test('updating package that has deps with peers', async (t: tape.Test) => {
  const project = prepare(t)

  await installPkgs(['abc-grand-parent-with-c@0'], testDefaults())
  await installPkgs(['abc-grand-parent-with-c@1'], testDefaults())

  t.pass('installation of latest did not fail')
})

test('updating shrinkwrap version 3 to 3.1', async (t: tape.Test) => {
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
          integrity: sha512-/sPoyuCaOuJAG6Gcq7HxiW8/++Jj3zmzfymr+mKbNG8VftROlRAd1qoOtA37xNJXYNRT2Zwb0Gym2fdt/eXKaQ==
      /abc-parent-with-ab/1.0.0/peer-c@1.0.0:
        dependencies:
          abc: /abc/1.0.0/165e1e08a3f7e7f77ddb572ad0e55660
          peer-a: 1.0.0
          peer-b: 1.0.0
        id: localhost+4873/abc-parent-with-ab/1.0.0
        resolution:
          integrity: sha512-8ULNWX/kq0K8zdbLdN9rjxJIVaqihDJbTTJSeH8cfz0rXleV2RxBhKJ9kqjk/kmplpHJEDyhLKDjubWlS10WUA==
      /abc/1.0.0/165e1e08a3f7e7f77ddb572ad0e55660:
        dependencies:
          dep-of-pkg-with-1-dep: 100.0.0
          peer-a: 1.0.0
          peer-b: 1.0.0
          peer-c: 1.0.0
        id: localhost+4873/abc/1.0.0
        resolution:
          integrity: sha512-PH3blWOnt6/jzbuoTHXRoV5jeBsIv+Xg0CyVmAarB/n086637teQj6hnCgGp2oc18ytYeNxjUAKM1jzm0CPSZA==
      /dep-of-pkg-with-1-dep/100.0.0:
        resolution:
          integrity: sha512-X7jXbtkdH5N79IYmVGSV3KHQjOo+RsbgO7xIQZ0OpOQlVzxoJ+e30l0G6STwgw1lgOOo5GQQ9C7VFSjnCaX1Sw==
      /peer-a/1.0.0:
        resolution:
          integrity: sha512-B8kajty4JNgNS7Oc82g9pY/nqUTjgMYzgXwbQL3NS2Mgi5asBcd5G3P0P38F8jdqhLy/OjYJ5FJlvmbJQX5azQ==
      /peer-b/1.0.0:
        resolution:
          integrity: sha512-MJi3M3Z34W8H97kn9wd0yV4sGStWuEm9eGhSqB7YpHi6sMIBGAVM8OAUyTNPHHoLvdZpbzcRScUWgkTqHpBdrQ==
      /peer-c/1.0.0:
        resolution:
          integrity: sha512-/bP9J3v+pIx6S6HWlZdWIHlmNOuiKSXZxxn7CXdjXwm8ypOtaEw6F08aXTxPDzFotpaRcdS6nN6K2DzA40XyPg==
    registry: 'http://localhost:4873/'
    shrinkwrapVersion: 3
    specifiers:
      abc-grand-parent-with-c: ^1.0.0
  `

  await fs.writeFile('shrinkwrap.yaml', shrV3Content, 'utf8')

  await install(testDefaults())

  const shr = await project.loadShrinkwrap()

  t.equal(shr.shrinkwrapMinorVersion, 4)
  t.ok(shr.packages['/abc/1.0.0/165e1e08a3f7e7f77ddb572ad0e55660'].peerDependencies)
})

test('pendingBuilds gets updated if install removes packages', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      'is-negative': '2.1.0',
      'sh-hello-world': '1.0.1',
    },
  })

  await install(testDefaults({ ignoreScripts: true }))
  const modules1 = await project.loadModules()

  await project.rewriteDependencies({
    'is-negative': '2.1.0',
  })

  await install(testDefaults({ ignoreScripts: true }))
  const modules2 = await project.loadModules()

  t.ok(modules1['pendingBuilds'].length > modules2['pendingBuilds'].length, 'pendingBuilds gets updated when install removes packages')
})
