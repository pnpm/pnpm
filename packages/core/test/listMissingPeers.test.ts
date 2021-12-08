import { listMissingPeers } from '@pnpm/core'
import { prepareEmpty } from '@pnpm/prepare'
import { testDefaults } from './utils'

test('cannot resolve peer dependency for top-level dependency', async () => {
  prepareEmpty()

  const peerDependencyIssues = await listMissingPeers([
    {
      manifest: {
        dependencies: {
          'ajv-keywords': '1.5.0',
        },
      },
      rootDir: process.cwd(),
    },
  ], await testDefaults())

  expect(peerDependencyIssues.length).toBe(1)
})
