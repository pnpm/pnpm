import { getPeerDependencyIssues } from '@pnpm/core'
import { prepareEmpty } from '@pnpm/prepare'
import { testDefaults } from './utils'

test('cannot resolve peer dependency for top-level dependency', async () => {
  prepareEmpty()

  const peerDependencyIssues = await getPeerDependencyIssues([
    {
      manifest: {
        dependencies: {
          'ajv-keywords': '1.5.0',
        },
      },
      rootDir: process.cwd(),
    },
  ], await testDefaults())

  expect(peerDependencyIssues['.'].missing).toHaveProperty('ajv')
})

test('a conflict is detected when the same peer is required with ranges that do not overlap', async () => {
  prepareEmpty()

  const peerDependencyIssues = await getPeerDependencyIssues([
    {
      manifest: {
        dependencies: {
          '@pnpm.e2e/has-foo100-peer': '1.0.0',
          '@pnpm.e2e/has-foo101-peer': '1.0.0',
        },
      },
      rootDir: process.cwd(),
    },
  ], await testDefaults())

  expect(peerDependencyIssues['.'].conflicts.length).toBe(1)
})
