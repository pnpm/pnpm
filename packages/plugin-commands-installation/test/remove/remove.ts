import PnpmError from '@pnpm/error'
import { remove } from '@pnpm/plugin-commands-installation'
import prepare, { preparePackages } from '@pnpm/prepare'
import test = require('tape')
import { DEFAULT_OPTS } from '../utils'

test('remove should fail if no dependency is specified for removal', async (t) => {
  prepare(t)

  let err!: PnpmError
  try {
    await remove.handler({
      ...DEFAULT_OPTS,
      dir: process.cwd(),
    }, [])
  } catch (_err) {
    err = _err
  }
  t.equal(err.code, 'ERR_PNPM_MUST_REMOVE_SOMETHING')
  t.equal(err.message, 'At least one dependency name should be specified for removal')
  t.end()
})
