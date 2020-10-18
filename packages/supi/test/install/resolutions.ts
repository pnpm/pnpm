import { prepareEmpty } from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import { addDependenciesToPackage } from 'supi'
import promisifyTape from 'tape-promise'
import {
  testDefaults,
} from '../utils'
import tape = require('tape')

const test = promisifyTape(tape)

test('versions are replaced with versions specified through resolutions field', async (t: tape.Test) => {
  const project = prepareEmpty(t)

  await addDistTag({ package: 'bar', version: '100.0.0', distTag: 'latest' })

  await addDependenciesToPackage({
    resolutions: {
      'bar@^100.0.0': '100.1.0',
      'dep-of-pkg-with-1-dep': '101.0.0',
    },
  }, ['pkg-with-1-dep@100.0.0', 'foobar@100.0.0'], await testDefaults())

  const lockfile = await project.readLockfile()
  t.ok(lockfile.packages['/dep-of-pkg-with-1-dep/101.0.0'])
  t.ok(lockfile.packages['/bar/100.1.0'])
})
