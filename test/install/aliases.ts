import {
  install,
  installPkgs,
  uninstall,
} from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import {
  addDistTag,
  prepare,
  testDefaults,
} from '../utils'

const test = promisifyTape(tape)
test.only = promisifyTape(tape.only)

test('installing aliased dependency', async (t: tape.Test) => {
  const project = prepare(t)
  await installPkgs(['negative@npm:is-negative@1.0.0', 'positive@npm:is-positive'], await testDefaults())

  const m = project.requireModule('negative')
  t.ok(typeof m === 'function', 'negative() is available')
  t.ok(typeof project.requireModule('positive') === 'function', 'positive() is available')

  t.deepEqual(await project.loadShrinkwrap(), {
    dependencies: {
      negative: '/is-negative/1.0.0',
      positive: '/is-positive/3.1.0',
    },
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
    registry: 'http://localhost:4873/',
    shrinkwrapMinorVersion: 5,
    shrinkwrapVersion: 3,
    specifiers: {
      negative: 'npm:is-negative@^1.0.0',
      positive: 'npm:is-positive@^3.1.0',
    },
  }, 'correct shrinkwrap.yaml')
})

test('aliased dependency w/o version spec, with custom tag config', async (t) => {
  const project = prepare(t)

  const tag = 'beta'

  await addDistTag('dep-of-pkg-with-1-dep', '100.1.0', 'latest')
  await addDistTag('dep-of-pkg-with-1-dep', '100.0.0', tag)

  await installPkgs(['foo@npm:dep-of-pkg-with-1-dep'], await testDefaults({tag}))

  await project.storeHas('dep-of-pkg-with-1-dep', '100.0.0')
})

test('a dependency has an aliased subdependency', async (t: tape.Test) => {
  const project = prepare(t)

  await installPkgs(['pkg-with-1-aliased-dep'], await testDefaults())

  t.equal(project.requireModule('pkg-with-1-aliased-dep')().name, 'dep-of-pkg-with-1-dep', 'can require aliased subdep')

  t.deepEqual(await project.loadShrinkwrap(), {
    dependencies: {
      'pkg-with-1-aliased-dep': '100.0.0',
    },
    packages: {
      '/dep-of-pkg-with-1-dep/100.1.0': {
        dev: false,
        resolution: {
          integrity: 'sha512-NrDz2149fygGT7uMe8Jj6rsgxZWuJQJqXfWk/gj5KWoxfRxmXkQZnPgOdoLnxCEq3RrKOotVcgUJtlM8fNRgvA==',
        },
      },
      '/pkg-with-1-aliased-dep/100.0.0': {
        dependencies: {
          dep: '/dep-of-pkg-with-1-dep/100.1.0',
        },
        dev: false,
        resolution: {
          integrity: 'sha512-zazvlUhlPW5Rr64YqOiZ9KRvPOcVI5ESbbBZ7obfDiwLwbI02EUX+Oo25D7GwTP0o2GoGPB3UkGdpz3HNQq0uw==',
        },
      },
    },
    registry: 'http://localhost:4873/',
    shrinkwrapMinorVersion: 5,
    shrinkwrapVersion: 3,
    specifiers: {
      'pkg-with-1-aliased-dep': '^100.0.0',
    },
  }, 'correct shrinkwrap.yaml')
})
