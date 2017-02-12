import tape = require('tape')
import promisifyTape from 'tape-promise'
import {prepare, testDefaults} from './utils'
import {installPkgs} from '../src'

const test = promisifyTape(tape)

test('shrinkwrap file has correct format', async t => {
  const project = prepare(t)

  await installPkgs(['pkg-with-1-dep'], testDefaults())

  const shr = await project.loadShrinkwrap()
  const id = 'localhost+4873/pkg-with-1-dep/100.0.0'

  t.equal(shr.version, 0, 'correct shrinkwrap version')

  t.ok(shr.dependencies, 'has dependencies field')
  t.equal(shr.dependencies['pkg-with-1-dep'], id, 'has dependency resolved')

  t.ok(shr.packages, 'has packages field')
  t.ok(shr.packages[id], `has resolution for ${id}`)
  t.ok(shr.packages[id].dependencies, `has dependency resolutions for ${id}`)
  t.ok(shr.packages[id].dependencies['dep-of-pkg-with-1-dep'], `has dependency resolved for ${id}`)
  t.ok(shr.packages[id].resolution, `has resolution for ${id}`)
  t.equal(shr.packages[id].resolution.tarball, 'http://localhost:4873/pkg-with-1-dep/-/pkg-with-1-dep-100.0.0.tgz', `has tarball for ${id}`)
})
