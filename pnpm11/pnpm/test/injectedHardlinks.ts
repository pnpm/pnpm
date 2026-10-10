import fs from 'node:fs'

import { expect, test } from '@jest/globals'
import { preparePackages } from '@pnpm/prepare'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm } from './utils/index.js'

test('frozen injected installs reuse hardlinks unless force is requested', async () => {
  preparePackages([
    {
      location: '.',
      package: {
        name: 'root',
        private: true,
        dependencies: { library: 'workspace:*' },
      },
    },
    { name: 'library', version: '1.0.0' },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['library'],
    injectWorkspacePackages: true,
    dedupeInjectedDeps: false,
    enableGlobalVirtualStore: false,
  })
  fs.writeFileSync('library/index.js', 'source-content')
  await execPnpm(['install', '--offline', '--no-frozen-lockfile'])
  const target = fs.realpathSync('node_modules/library')
  const original = fs.statSync(target, { bigint: true }).ino
  await execPnpm(['install', '--offline', '--frozen-lockfile'])
  expect(fs.statSync(target, { bigint: true }).ino).toBe(original)
  await execPnpm(['install', '--offline', '--frozen-lockfile', '--force'])
  expect(fs.statSync(target, { bigint: true }).ino).not.toBe(original)
  expect(fs.readFileSync(`${target}/index.js`, 'utf8')).toBe('source-content')
})
