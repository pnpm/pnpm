import fs from 'node:fs'

import { prepare } from '@pnpm/prepare'

import { execPnpm } from '../utils/index.js'

// Skipped: the registry-mock package was published with the old transformEngines behavior
// that moved engines.runtime to devEngines.runtime. Unskip after republishing registry-mock.
test.skip('installing a CLI tool that requires a specific version of Node.js to be installed alongside it', async () => {
  prepare()
  fs.writeFileSync('pnpm-workspace.yaml', 'allowBuilds: { "@pnpm.e2e/cli-with-node-engine@1.0.0": true }', 'utf8')

  await execPnpm(['add', '@pnpm.e2e/cli-with-node-engine@1.0.0'])
  await execPnpm(['exec', 'cli-with-node-engine'])
  expect(fs.readFileSync('node-version', 'utf8')).toBe('v22.19.0')
})
