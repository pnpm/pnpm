import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'

import { execPnpmSync } from './utils/index.js'

const progressDisabledCases: Array<{
  name: string
  args: string[]
  env?: Record<string, string>
  workspaceConfig?: string
}> = [
  { name: '--no-progress', args: ['install', '--no-progress'] },
  { name: 'PNPM_CONFIG_PROGRESS=false', args: ['install'], env: { PNPM_CONFIG_PROGRESS: 'false' } },
  { name: 'progress: false', args: ['install'], workspaceConfig: 'progress: false\n' },
]

test.each(progressDisabledCases)('$name disables progress output', ({ args, env, workspaceConfig }) => {
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

test('--progress overrides progress: false', () => {
  prepare({
    dependencies: {
      'is-positive': '1.0.0',
    },
  })
  fs.writeFileSync('pnpm-workspace.yaml', 'progress: false\n', 'utf8')

  const result = execPnpmSync(['install', '--progress', '--reporter=append-only'], {
    expectSuccess: true,
  })
  const output = result.stdout.toString() + result.stderr.toString()

  expect(output).toContain('Progress:')
})
