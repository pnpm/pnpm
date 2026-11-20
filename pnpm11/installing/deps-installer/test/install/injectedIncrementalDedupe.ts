import path from 'node:path'

import { expect, test } from '@jest/globals'
import { assertProject } from '@pnpm/assert-project'
import { type MutatedProject, mutateModules } from '@pnpm/installing.deps-installer'
import { preparePackages } from '@pnpm/prepare'
import type { ProjectManifest, ProjectRootDir } from '@pnpm/types'

import { testDefaults } from '../utils/index.js'

// https://github.com/pnpm/pnpm/issues/17094
test('an injected workspace dependency dedupes to a link when an install leaves the workspace package unresolved', async () => {
  preparePackages([
    { location: 'provider', package: { name: 'provider', version: '1.0.0' } },
    { location: 'app', package: { name: 'app', version: '1.0.0' } },
  ])
  const install = async (manifests: Record<string, ProjectManifest>) => {
    const allProjects = Object.entries(manifests).map(([dir, manifest]) => ({ buildIndex: 0, manifest, rootDir: path.resolve(dir) as ProjectRootDir }))
    const mutations: MutatedProject[] = allProjects.map(({ rootDir }) => ({ mutation: 'install', rootDir }))
    await mutateModules(mutations, testDefaults({ allProjects, autoInstallPeers: false, dedupePeerDependents: false, injectWorkspacePackages: true }))
  }
  const provider = { name: 'provider', version: '1.0.0', dependencies: { '@pnpm.e2e/peer-c': '1.0.0' } }
  await install({ provider, app: { name: 'app', version: '1.0.0' } })
  await install({ provider, app: { name: 'app', version: '1.0.0', dependencies: { provider: 'workspace:*' } } })

  expect(assertProject(process.cwd()).readLockfile().importers['app'].dependencies?.provider.version).toBe('link:../provider')
})
