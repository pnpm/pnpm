import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { prepare } from '@pnpm/prepare'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm } from '../utils/index.js'

test.each(['plain', 'override', 'alias', 'different-version'])(
  'optional workspace peer keeps its selected provider during install and dedupe: %s',
  async (variant) => {
    const project = prepare({
      dependencies: variant === 'alias'
        ? { positive: 'workspace:is-positive@*' }
        : { 'is-positive': 'workspace:*' },
    })
    writeYamlFileSync('pnpm-workspace.yaml', {
      packages: ['packages/*'],
      ...(variant === 'override' ? { overrides: { 'is-positive': 'workspace:is-positive@*' } } : {}),
    })
    writeManifest('packages/is-positive', {
      name: 'is-positive',
      version: '1.0.0',
      dependencies: { 'peer-consumer': 'file:../../fixtures/peer-consumer' },
    })
    writeManifest('fixtures/peer-consumer', {
      name: 'peer-consumer',
      version: '1.0.0',
      peerDependencies: { 'is-positive': variant === 'different-version' ? '^2.0.0' : '*' },
      peerDependenciesMeta: { 'is-positive': { optional: true } },
    })
    if (variant === 'different-version') {
      writeManifest('packages/other', {
        name: 'other',
        version: '1.0.0',
        dependencies: { 'is-positive': '2.0.0' },
      })
    }

    const assertProvider = (): void => {
      const consumer = fs.realpathSync('packages/is-positive/node_modules/peer-consumer')
      const peer = fs.realpathSync(path.join(path.dirname(consumer), 'is-positive'))
      const expected = variant === 'different-version'
        ? 'packages/other/node_modules/is-positive'
        : 'packages/is-positive'
      expect(peer).toBe(fs.realpathSync(expected))
      if (variant !== 'different-version') {
        expect(Object.keys(project.readLockfile().snapshots)).not.toContain('is-positive@1.0.0')
      }
    }

    await execPnpm(['install'])
    assertProvider()
    await execPnpm(['dedupe'])
    assertProvider()
  }
)

function writeManifest (dir: string, manifest: object): void {
  fs.mkdirSync(dir, { recursive: true })
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify(manifest))
}
