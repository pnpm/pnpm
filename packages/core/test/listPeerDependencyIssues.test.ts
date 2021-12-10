import { listPeerDependencyIssues } from '@pnpm/core'
import { prepareEmpty } from '@pnpm/prepare'
import { testDefaults } from './utils'

test('cannot resolve peer dependency for top-level dependency', async () => {
  prepareEmpty()

  const peerDependencyIssues = await listPeerDependencyIssues([
    {
      manifest: {
        dependencies: {
          'ajv-keywords': '1.5.0',
        },
      },
      rootDir: process.cwd(),
    },
  ], await testDefaults())

  expect(peerDependencyIssues.issues.missing).toHaveProperty('ajv')
})

test('a conflict is detected when the same peer is required with ranges that do not overlap', async () => {
  prepareEmpty()

  const peerDependencyIssues = await listPeerDependencyIssues([
    {
      manifest: {
        dependencies: {
          'has-foo100-peer': '1.0.0',
          'has-foo101-peer': '1.0.0',
        },
      },
      rootDir: process.cwd(),
    },
  ], await testDefaults())

  expect(peerDependencyIssues.conflicts.length).toBe(1)
})
