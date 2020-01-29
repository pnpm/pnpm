import { install, update } from '@pnpm/plugin-commands-installation'
import prepare from '@pnpm/prepare'
import { addDistTag } from '@pnpm/registry-mock'
import test = require('tape')
import { DEFAULT_OPTS } from '../utils'

test('update with "*" pattern', async (t) => {
  await addDistTag({ package: 'peer-a', version: '1.0.1', distTag: 'latest' })
  await addDistTag({ package: 'peer-c', version: '2.0.0', distTag: 'latest' })
  await addDistTag({ package: 'pnpm-foo', version: '2.0.0', distTag: 'latest' })

  const project = prepare(t, {
    dependencies: {
      'peer-a': '1.0.0',
      'peer-c': '1.0.0',
      'pnpm-foo': '1.0.0',
    },
  })

  await install.handler([], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    workspaceDir: process.cwd(),
  })

  await update.handler(['peer-*'], {
    ...DEFAULT_OPTS,
    dir: process.cwd(),
    latest: true,
    workspaceDir: process.cwd(),
  })

  const lockfile = await project.readLockfile()

  t.ok(lockfile.packages['/peer-a/1.0.1'])
  t.ok(lockfile.packages['/peer-c/2.0.0'])
  t.ok(lockfile.packages['/pnpm-foo/1.0.0'])
  t.end()
})
