import prepare from '@pnpm/prepare'
import promisifyTape from 'tape-promise'
import { execPnpm } from '../utils'
import deepRequireCwd = require('deep-require-cwd')
import tape = require('tape')

const test = promisifyTape(tape)

test('installing optional dependencies when --no-optional is not used', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      'pkg-with-good-optional': '*',
    },
    optionalDependencies: {
      'is-positive': '1.0.0',
    },
  })

  await execPnpm(['install'])

  await project.has('is-positive')
  await project.has('pkg-with-good-optional')

  t.ok(deepRequireCwd(['pkg-with-good-optional', 'dep-of-pkg-with-1-dep', './package.json']))
  t.ok(deepRequireCwd(['pkg-with-good-optional', 'is-positive', './package.json']), 'optional subdep installed')
})

test('not installing optional dependencies when --no-optional is used', async (t: tape.Test) => {
  const project = prepare(t, {
    dependencies: {
      'pkg-with-good-optional': '*',
    },
    optionalDependencies: {
      'is-positive': '1.0.0',
    },
  })

  await execPnpm(['install', '--no-optional'])

  await project.hasNot('is-positive')
  await project.has('pkg-with-good-optional')

  t.ok(deepRequireCwd(['pkg-with-good-optional', 'dep-of-pkg-with-1-dep', './package.json']))
  t.notOk(deepRequireCwd.silent(['pkg-with-good-optional', 'is-positive', './package.json']), 'optional subdep not installed')
})
