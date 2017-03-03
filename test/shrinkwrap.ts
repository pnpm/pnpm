import tape = require('tape')
import promisifyTape from 'tape-promise'
import writeYamlFile = require('write-yaml-file')
import exists = require('path-exists')
import {prepare, testDefaults} from './utils'
import {installPkgs, install} from '../src'

const test = promisifyTape(tape)

test('shrinkwrap file has correct format', async t => {
  const project = prepare(t)

  await installPkgs(['pkg-with-1-dep'], testDefaults({save: true}))

  const shr = await project.loadShrinkwrap()
  const id = 'localhost+4873/pkg-with-1-dep/100.0.0'

  t.equal(shr.version, 1, 'correct shrinkwrap version')

  t.ok(shr.dependencies, 'has dependencies field')
  t.equal(shr.dependencies['pkg-with-1-dep@^100.0.0'], id, 'has dependency resolved')

  t.ok(shr.packages, 'has packages field')
  t.ok(shr.packages[id], `has resolution for ${id}`)
  t.ok(shr.packages[id].dependencies, `has dependency resolutions for ${id}`)
  t.ok(shr.packages[id].dependencies['dep-of-pkg-with-1-dep'], `has dependency resolved for ${id}`)
  t.ok(shr.packages[id].resolution, `has resolution for ${id}`)
  t.equal(shr.packages[id].resolution.tarball, 'http://localhost:4873/pkg-with-1-dep/-/pkg-with-1-dep-100.0.0.tgz', `has tarball for ${id}`)
})

test('fail when shasum from shrinkwrap does not match with the actual one', async t => {
  const project = prepare(t, {
    dependencies: {
      'is-negative': '2.1.0',
    },
  })

  await writeYamlFile('shrinkwrap.yaml', {
    version: 1,
    dependencies: {
      'is-negative@2.1.0': 'localhost+4873/is-negative/2.1.0',
    },
    packages: {
      'localhost+4873/is-negative/2.1.0': {
        resolution: {
          shasum: '00000000000000000000000000000000000000000',
          tarball: 'http://localhost:4873/is-negative/-/is-negative-2.1.0.tgz',
        },
      },
    },
  })

  try {
    await install(testDefaults())
    t.fail('installation should have failed')
  } catch (err) {
    t.ok(err.message.indexOf('Incorrect shasum') !== -1, 'failed with expected error')
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

  await installPkgs(['pkg-with-1-dep'], testDefaults({save: false}))

  t.ok(!await project.loadShrinkwrap(), 'shrinkwrap file not created')
})
