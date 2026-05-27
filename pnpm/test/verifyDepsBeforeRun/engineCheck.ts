import { expect, test } from '@jest/globals'
import { preparePackages } from '@pnpm/prepare'
import type { ProjectManifest } from '@pnpm/types'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm, execPnpmSync } from '../utils/index.js'

test('verify-deps-before-run does not emit unsupported engine warnings for workspace projects', async () => {
  const manifests: Record<string, ProjectManifest> = {
    root: {
      name: 'root',
      private: true,
      scripts: {
        start: 'echo hello from root',
      },
    },
    'has-strict-engine': {
      name: 'has-strict-engine',
      private: true,
      engines: {
        node: '>=99.0.0',
      },
      scripts: {
        start: 'echo hello from has-strict-engine',
      },
    },
  }

  preparePackages([
    { location: '.', package: manifests.root },
    manifests['has-strict-engine'],
  ])

  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })

  await execPnpm(['install'])

  const { stdout, stderr } = execPnpmSync(['--config.verify-deps-before-run=install', 'start'], {
    expectSuccess: true,
  })
  expect(stdout.toString()).toContain('hello from root')
  expect(stdout.toString() + stderr.toString()).not.toContain('Unsupported engine')
})
