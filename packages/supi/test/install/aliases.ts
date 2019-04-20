import { WANTED_LOCKFILE } from '@pnpm/constants'
import { prepareEmpty } from '@pnpm/prepare'
import { getIntegrity } from 'pnpm-registry-mock'
import { addDependenciesToPackage } from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import {
  addDistTag,
  testDefaults,
} from '../utils'

const test = promisifyTape(tape)
const testOnly = promisifyTape(tape.only)

test('installing aliased dependency', async (t: tape.Test) => {
  const project = prepareEmpty(t)
  await addDependenciesToPackage({}, ['negative@npm:is-negative@1.0.0', 'positive@npm:is-positive'], await testDefaults())

  const m = project.requireModule('negative')
  t.ok(typeof m === 'function', 'negative() is available')
  t.ok(typeof project.requireModule('positive') === 'function', 'positive() is available')

  t.deepEqual(await project.readLockfile(), {
    dependencies: {
      negative: '/is-negative/1.0.0',
      positive: '/is-positive/3.1.0',
    },
    lockfileVersion: 5,
    packages: {
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
      negative: 'npm:is-negative@1.0.0',
      positive: 'npm:is-positive@^3.1.0',
    },
  }, `correct ${WANTED_LOCKFILE}`)
})

test('aliased dependency w/o version spec, with custom tag config', async (t) => {
  const project = prepareEmpty(t)

  const tag = 'beta'

  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')
  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', tag)

  await addDependenciesToPackage({}, ['foo@npm:dep-of-pkg-with-1-dep'], await testDefaults({ tag }))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
})

test('a dependency has an aliased subdependency', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDependenciesToPackage({}, ['pkg-with-1-aliased-dep'], await testDefaults())

  t.equal(project.requireModule('pkg-with-1-aliased-dep')().name, 'dep-of-pkg-with-1-dep', 'can require aliased subdep')

  t.deepEqual(await project.readLockfile(), {
    dependencies: {
      'pkg-with-1-aliased-dep': '100.0.0',
    },
    lockfileVersion: 5,
    packages: {
      '/dep-of-pkg-with-1-dep/100.1.0': {
        dev: false,
        resolution: {
          integrity: getIntegrity('dep-of-pkg-with-1-dep', '100.1.0'),
        },
      },
      '/pkg-with-1-aliased-dep/100.0.0': {
        dependencies: {
          dep: '/dep-of-pkg-with-1-dep/100.1.0',
        },
        dev: false,
        resolution: {
          integrity: getIntegrity('pkg-with-1-aliased-dep', '100.0.0'),
        },
      },
    },
    specifiers: {
      'pkg-with-1-aliased-dep': '^100.0.0',
    },
  }, `correct ${WANTED_LOCKFILE}`)
})
