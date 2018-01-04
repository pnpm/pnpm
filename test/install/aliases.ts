import tape = require('tape')
import promisifyTape from 'tape-promise'
import {
  prepare,
  addDistTag,
  testDefaults,
} from '../utils'
import {
  install,
  installPkgs,
  uninstall,
} from 'supi'

const test = promisifyTape(tape)

test('installing aliased dependency', async (t: tape.Test) => {
  const project = prepare(t)
  await installPkgs(['negative@npm:is-negative@1.0.0'], await testDefaults())

  const m = project.requireModule('negative')
  t.ok(typeof m === 'function', 'negative() is available')

  t.deepEqual(await project.loadShrinkwrap(), {
    dependencies: {
      negative: '/is-negative/1.0.0',
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
    },
    registry: 'http://localhost:4873/',
    shrinkwrapMinorVersion: 4,
    shrinkwrapVersion: 3,
    specifiers: {
      negative: 'npm:is-negative@^1.0.0',
    },
  }, 'correct shrinkwrap.yaml')
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
          integrity: 'sha512-01GGXw18uxujjxUU18Hhc7iRjMsZhUBB7gS+dVQBo0WPZBVcCmIe0TN4z9jvTxqglDAqDnznCiYAroYcQ7mZww==',
        },
      },
      '/pkg-with-1-aliased-dep/100.0.0': {
        dependencies: {
          dep: '/dep-of-pkg-with-1-dep/100.1.0',
        },
        dev: false,
        resolution: {
          integrity: 'sha512-js3vHxmy+JzgbgmxF8tK4rlIPLa2WO7T3zhL1AHPntEzLZT7tWX5WKDEb9sYZprVHNpZyNm+4UP0RmbX7CTdyA==',
        },
      },
    },
    registry: 'http://localhost:4873/',
    shrinkwrapMinorVersion: 4,
    shrinkwrapVersion: 3,
    specifiers: {
      'pkg-with-1-aliased-dep': '^100.0.0',
    },
  }, 'correct shrinkwrap.yaml')
})
