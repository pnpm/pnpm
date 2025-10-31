import fs from 'fs'
import { prepare } from '@pnpm/prepare'
import { execPnpm } from '../utils/index.js'

test('installing a CLI tool that requires a specific version of Node.js to be installed alongside it', async () => {
  prepare()

  await execPnpm(['add', '@pnpm.e2e/cli-with-node-engine@1.0.0'])
  await execPnpm(['exec', 'cli-with-node-engine'])
  expect(fs.readFileSync('node-version', 'utf8')).toBe('v22.19.0')
})
