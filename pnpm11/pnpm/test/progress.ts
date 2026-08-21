import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'

import { execPnpmSync } from './utils/index.js'

test.each([
  { name: '--no-progress', args: ['install', '--no-progress'], env: {} },
  { name: 'PNPM_CONFIG_PROGRESS=false', args: ['install'], env: { PNPM_CONFIG_PROGRESS: 'false' } },
  { name: 'progress: false', args: ['install'], env: {}, workspaceConfig: 'progress: false\n' },
])('$name disables progress output', ({ args, env, workspaceConfig }) => {
  prepare({
    dependencies: {
      'is-positive': '1.0.0',
    },
  })
  fs.writeFileSync('pnpm-workspace.yaml', workspaceConfig ?? '{}\n', 'utf8')

  const result = execPnpmSync([...args, '--reporter=append-only'], {
    env,
    expectSuccess: true,
  })
  const output = result.stdout.toString() + result.stderr.toString()

  expect(output).not.toContain('Progress:')
  expect(output).toContain('is-positive 1.0.0')
})
