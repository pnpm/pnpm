import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'

import { execPnpm, execPnpmSync } from './utils/index.js'

test('pnpm node -v prints the Node version', () => {
  prepare()

  const result = execPnpmSync(['node', '-v'])

  expect(result.status).toBe(0)
  // -v output is `v<major>.<minor>.<patch>` — assert the shape, not the exact
  // version (whichever node binary cross-spawn falls back to may differ).
  expect(String(result.stdout).trim()).toMatch(/^v\d+\.\d+\.\d+/)
})

test('pnpm node -e runs the passed expression', () => {
  prepare()

  const result = execPnpmSync(['node', '-e', 'console.log("PNPM_NODE_E")'])

  expect(result.status).toBe(0)
  expect(String(result.stdout)).toContain('PNPM_NODE_E')
})

test('pnpm node defers to a same-named script in package.json', () => {
  prepare({
    scripts: {
      node: 'echo SCRIPT_RAN',
    },
  })

  const result = execPnpmSync(['node'])

  expect(result.status).toBe(0)
  expect(String(result.stdout)).toContain('SCRIPT_RAN')
})

test('pnpm node uses the runtime installed in node_modules over PATH', async () => {
  prepare({
    devEngines: {
      runtime: {
        name: 'node',
        version: '22.20.0',
        onFail: 'download',
      },
    },
  })
  // Make the global-bin-dir lookup miss so the project-level resolution
  // is what serves this call (not the host's globally installed runtime).
  fs.writeFileSync('.npmrc', 'global-bin-dir=does-not-exist\n', 'utf8')

  await execPnpm(['install'])

  const result = execPnpmSync(['node', '-v'])

  expect(result.status).toBe(0)
  expect(String(result.stdout).trim()).toBe('v22.20.0')
})
