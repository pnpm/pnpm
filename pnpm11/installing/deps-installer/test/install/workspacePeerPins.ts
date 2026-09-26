import path from 'node:path'

import { expect, jest, test } from '@jest/globals'
import { assertProject } from '@pnpm/assert-project'
import { mutateModules } from '@pnpm/installing.deps-installer'
import { prepareEmpty } from '@pnpm/prepare'
import type { ProjectRootDir } from '@pnpm/types'

import { testDefaults } from '../utils/index.js'

// https://github.com/pnpm/pnpm/issues/11800
test.each([
  { bumpedSpec: '2.0.0', appVersion: '2.0.0', peerRange: '>=1.0.0', expected: '2.0.0' },
  { bumpedSpec: '^3.0.0', appVersion: '3.1.0', peerRange: '>=1.0.0', expected: '3.1.0' },
  { bumpedSpec: '2.0.0', appVersion: '2.0.0', peerRange: '^1.0.0', expected: '1.0.0' },
])('an auto-installed peer with range $peerRange resolves to $expected after another workspace project moves to $bumpedSpec', async ({ bumpedSpec, appVersion, peerRange, expected }) => {
  prepareEmpty()
  const mutations = ['app', 'lib'].map((name) => ({ mutation: 'install' as const, rootDir: path.resolve(name) as ProjectRootDir }))
  const lockfileDefaults = { autoInstallPeers: true, lockfileOnly: true }

  await mutateModules(mutations, testDefaults({ ...lockfileDefaults, allProjects: createPeerProviderProjects('1.0.0', peerRange) }))
  const project = assertProject(process.cwd())
  expect(project.readLockfile().importers['lib'].dependencies).toStrictEqual({
    'is-positive': { specifier: peerRange, version: '1.0.0' },
  })

  const reporter = jest.fn()
  await mutateModules(mutations, testDefaults({ ...lockfileDefaults, allProjects: createPeerProviderProjects(bumpedSpec, peerRange), reporter }))
  expect(reporter).not.toHaveBeenCalledWith(expect.objectContaining({ level: 'warn' }))
  const lockfile = project.readLockfile()
  expect(lockfile.importers['app'].dependencies).toStrictEqual({
    'is-positive': { specifier: bumpedSpec, version: appVersion },
  })
  expect(lockfile.importers['lib'].dependencies).toStrictEqual({
    'is-positive': { specifier: peerRange, version: expected },
  })
})

test('a filtered install of the providing project also moves the lockfile entry of an unselected project\'s auto-installed peer', async () => {
  prepareEmpty()
  const [appMutation, libMutation] = ['app', 'lib'].map((name) => ({ mutation: 'install' as const, rootDir: path.resolve(name) as ProjectRootDir }))
  await mutateModules([appMutation, libMutation], testDefaults({ autoInstallPeers: true, allProjects: createPeerProviderProjects('1.0.0', '>=1.0.0') }))

  await mutateModules([appMutation], testDefaults({ autoInstallPeers: true, allProjects: createPeerProviderProjects('2.0.0', '>=1.0.0') }))

  const project = assertProject(process.cwd())
  const lockfile = project.readLockfile()
  expect(lockfile.importers['app'].dependencies).toStrictEqual({
    'is-positive': { specifier: '2.0.0', version: '2.0.0' },
  })
  expect(lockfile.importers['lib'].dependencies).toStrictEqual({
    'is-positive': { specifier: '>=1.0.0', version: '2.0.0' },
  })
})

function createPeerProviderProjects (appSpec: string, peerRange: string) {
  return [
    {
      buildIndex: 0,
      manifest: { name: 'app', version: '1.0.0', dependencies: { 'is-positive': appSpec } },
      rootDir: path.resolve('app') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: { name: 'lib', version: '1.0.0', peerDependencies: { 'is-positive': peerRange } },
      rootDir: path.resolve('lib') as ProjectRootDir,
    },
  ]
}
