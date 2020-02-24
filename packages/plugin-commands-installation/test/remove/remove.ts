import PnpmError from '@pnpm/error'
import { install, remove } from '@pnpm/plugin-commands-installation'
import prepare from '@pnpm/prepare'
import test = require('tape')
import { DEFAULT_OPTS } from '../utils'

test('remove: fail when package not in dependencies', async (t) => {
  prepare(t, {
    dependencies: {
      'peer-a': '1.0.0',
      'peer-c': '1.0.0',
    },
  })

  await install.handler([], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    workspaceDir: process.cwd(),
  })

  let err!: PnpmError
  try {
    await remove.handler(['peer-b'], {
      ...DEFAULT_OPTS,
      dir: process.cwd(),
      sharedWorkspaceLockfile: true,
      workspaceDir: process.cwd(),
    })
  } catch (_err) {
    err = _err
  }
  t.equal(err.code, 'ERR_PNPM_NO_PACKAGE_IN_DEPENDENCY')
  t.equal(err.message, 'None of the specified packages were found in the dependencies.')
  t.end()
})
