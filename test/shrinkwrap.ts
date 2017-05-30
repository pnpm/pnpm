import tape = require('tape')
import promisifyTape from 'tape-promise'
import writeYamlFile = require('write-yaml-file')
import exists = require('path-exists')
import {prepare, testDefaults, addDistTag} from './utils'
import {installPkgs, install} from '../src'
import readPkg = require('read-pkg')
import writePkg = require('write-pkg')
import rimraf = require('rimraf-then')

const test = promisifyTape(tape)

test('shrinkwrap file has correct format', async t => {
  const project = prepare(t)

  await installPkgs(['pkg-with-1-dep', '@rstacruz/tap-spec@4.1.1', 'kevva/is-negative'], testDefaults({save: true}))

  const shr = await project.loadShrinkwrap()
  const id = '/pkg-with-1-dep/100.0.0'

  t.equal(shr.version, 3, 'correct shrinkwrap version')

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

  t.ok(shr.dependencies, 'has dependencies field')

  t.equal(shr.dependencies['is-negative'], '2.1.0', 'has dependency resolved')

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

test("shrinkwrap doesn't lock subdependencies that don't satisfy the new specs", async t => {
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

test('shrinkwrap not created when no deps in package.json', async t => {
  const project = prepare(t)

  await install(testDefaults())

  t.ok(!await project.loadShrinkwrap(), 'shrinkwrap file not created')
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

  t.ok(!await project.loadShrinkwrap(), 'shrinkwrap file removed')
})

test('respects shrinkwrap.yaml for top dependencies', async (t: tape.Test) => {
  const project = prepare(t)

  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', 'latest')

  await installPkgs(['dep-of-pkg-with-1-dep'], testDefaults({save: true}))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')

  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')

  await install(testDefaults())

  await project.storeHasNot('dep-of-pkg-with-1-dep', '100.1.0')
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
      integrity: 'sha1-9p48+xLRmGikMywRAuZ9AOkCYDY=',
    },
  }

  shr.packages['/pkg-with-1-dep/100.0.0']['dependencies']['dep-of-pkg-with-1-dep'] = '100.1.0'

  await writeYamlFile('shrinkwrap.yaml', shr)

  await install(testDefaults())

  await project.storeHas('dep-of-pkg-with-1-dep', '100.1.0')
})

test("recreates shrinkwrap file if it doesn't match the dependencies in package.json", async (t: tape.Test) => {
  const project = prepare(t)

  await installPkgs(['is-negative@2.0.0'], testDefaults({saveExact: true}))

  const shr1 = await project.loadShrinkwrap()
  t.equal(shr1.dependencies['is-negative'], '2.0.0')
  t.equal(shr1.specifiers['is-negative'], '2.0.0')

  const pkg = await readPkg()

  pkg.dependencies['is-negative'] = '^2.1.0'

  await writePkg(pkg)

  await install(testDefaults())

  const shr = await project.loadShrinkwrap()

  t.equal(shr.dependencies['is-negative'], '2.1.0')
  t.equal(shr.specifiers['is-negative'], '^2.1.0')
})

test('repeat install with shrinkwrap should not mutate shrinkwrap when dependency has version specified with v prefix', async (t: tape.Test) => {
  const project = prepare(t)

  await installPkgs(['highmaps-release@5.0.11'], testDefaults())

  const shr1 = await project.loadShrinkwrap()

  t.equal(shr1.dependencies['highmaps-release'], '5.0.11')

  await rimraf('node_modules')

  await install(testDefaults())

  const shr2 = await project.loadShrinkwrap()

  t.deepEqual(shr1, shr2)
})

test('package is not marked dev if it is also a subdep of a regular dependency', async (t: tape.Test) => {
  const project = prepare(t)

  await installPkgs(['pkg-with-1-dep'])
  await installPkgs(['dep-of-pkg-with-1-dep'], {saveDev: true})

  const shr = await project.loadShrinkwrap()

  t.notOk(shr.packages['/dep-of-pkg-with-1-dep/1.1.0']['dev'])
})

test('package is not marked optional if it is also a subdep of a regular dependency', async (t: tape.Test) => {
  const project = prepare(t)

  await installPkgs(['pkg-with-1-dep'])
  await installPkgs(['dep-of-pkg-with-1-dep'], {saveOptional: true})

  const shr = await project.loadShrinkwrap()

  t.notOk(shr.packages['/dep-of-pkg-with-1-dep/1.1.0']['optional'])
})
