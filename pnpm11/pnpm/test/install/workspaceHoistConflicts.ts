import fs from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import { preparePackages } from '@pnpm/prepare'
import { writeYamlFileSync } from 'write-yaml-file'

import { execPnpm, execPnpmSync } from '../utils/index.js'

test.each([
  { publicHoist: false, filter: [] },
  { publicHoist: true, filter: [] },
  { publicHoist: false, filter: ['--filter', 'consumer'] },
  { publicHoist: true, filter: ['--filter', 'consumer'] },
])('frozen install and dedupe preserve a dependency sharing a workspace name (public hoist: $publicHoist, filter: $filter)', async ({ publicHoist, filter }) => {
  preparePackages([
    { location: '.', package: { name: 'root', private: true } },
    { location: 'packages/shared', package: { name: 'shared', version: '1.0.0' } },
    { location: 'packages/consumer', package: { name: 'consumer', dependencies: { shared: 'workspace:*' } } },
    { location: 'packages/external', package: { name: 'external', dependencies: { shared: 'file:../../shared-1.0.0.tgz' } } },
    { location: 'registry', package: { name: 'shared', version: '1.0.0' } },
  ])
  fs.writeFileSync('registry/index.js', "module.exports = 'external'\n")
  fs.writeFileSync('packages/shared/index.js', "module.exports = 'workspace'\n")
  execPnpmSync(['pack', '--pack-destination', '..'], { cwd: path.resolve('registry'), expectSuccess: true })
  writeYamlFileSync('pnpm-workspace.yaml', {
    packages: ['packages/*'],
    linkWorkspacePackages: false,
    publicHoistPattern: publicHoist ? ['*'] : [],
  })
  const requireFromVirtualStore = createRequire(path.resolve('node_modules/.pnpm/tool/node_modules/tool/index.js'))
  const hoistedSharedIndex = path.join(publicHoist ? 'node_modules' : 'node_modules/.pnpm/node_modules', 'shared/index.js')

  await execPnpm(['install', '--offline'])
  expect(requireFromVirtualStore('shared')).toBe('external')
  const hoistedPackage = requireFromVirtualStore.resolve('shared')

  await execPnpm(['install', '--frozen-lockfile', '--offline', ...filter])
  expect(fs.realpathSync(hoistedSharedIndex)).toBe(hoistedPackage)

  await execPnpm(['dedupe', '--offline'])
  expect(fs.realpathSync(hoistedSharedIndex)).toBe(hoistedPackage)
})

test('--no-runtime does not reserve an unselected runtime alias from workspace hoisting', async () => {
  const projects = preparePackages([
    { location: '.', package: { name: 'root', private: true } },
    { location: 'packages/node', package: { name: 'node', version: '1.0.0' } },
    { location: 'packages/consumer', package: { name: 'consumer', dependencies: { node: 'workspace:*' } } },
    {
      location: 'packages/runtime-user',
      package: {
        name: 'runtime-user',
        devEngines: { runtime: { name: 'node', version: '24.0.0', onFail: 'download' } },
      },
    },
  ])
  writeYamlFileSync('pnpm-workspace.yaml', { packages: ['packages/*'] })

  await execPnpm(['install', '--lockfile-only'])
  await execPnpm(['install', '--frozen-lockfile', '--no-runtime', '--filter', 'consumer'])

  expect(fs.realpathSync('node_modules/.pnpm/node_modules/node')).toBe(path.resolve('packages/node'))
  projects.consumer.has('node')
})
