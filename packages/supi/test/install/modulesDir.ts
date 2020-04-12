import { prepareEmpty } from '@pnpm/prepare'
import rimraf = require('@zkochan/rimraf')
import { install } from 'supi'
import tape = require('tape')
import promisifyTape from 'tape-promise'
import { testDefaults } from '../utils'

const test = promisifyTape(tape)

test('installing to a custom modules directory', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await install({
    dependencies: {
      'is-positive': '1.0.0',
    },
  }, await testDefaults({ modulesDir: 'pnpm_modules' }))

  await project.has('is-positive', 'pnpm_modules')

  await rimraf('pnpm_modules')
  await project.hasNot('is-positive', 'pnpm_modules')

  await install({
    dependencies: {
      'is-positive': '1.0.0',
    },
  }, await testDefaults({ frozenLockfile: true, modulesDir: 'pnpm_modules' }))

  await project.has('is-positive', 'pnpm_modules')
})
