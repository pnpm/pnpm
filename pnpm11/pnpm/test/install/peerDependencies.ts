import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'

import { execPnpm } from '../utils/index.js'

test('transitive pending peer uses provider final suffix in lockfile', async () => {
  const project = prepare({
    dependencies: {
      '@pnpm.e2e/final-peer-a': '1.0.0',
      '@pnpm.e2e/final-peer-c': '1.0.0',
    },
  })

  await execPnpm(['install'])

  const lockfile = project.readLockfile()
  const snapshots = Object.keys(lockfile.snapshots)
  const expected = '@pnpm.e2e/final-peer-x@1.0.0(@pnpm.e2e/final-peer-b@1.0.0(@pnpm.e2e/final-peer-a@1.0.0(@pnpm.e2e/final-peer-c@1.0.0)))'
  const provisional = '@pnpm.e2e/final-peer-x@1.0.0(@pnpm.e2e/final-peer-b@1.0.0(@pnpm.e2e/final-peer-a@1.0.0))'

  expect(snapshots).toContain(expected)
  expect(snapshots).not.toContain(provisional)
})
