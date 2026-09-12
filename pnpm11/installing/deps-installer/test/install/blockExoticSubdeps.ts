import path from 'node:path'

import { afterEach, beforeEach, expect, test } from '@jest/globals'
import { addDependenciesToPackage, type MutatedProject, mutateModules } from '@pnpm/installing.deps-installer'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import { getMockAgent, setupMockAgent, teardownMockAgent } from '@pnpm/testing.mock-agent'
import type { ProjectRootDir } from '@pnpm/types'

import { testDefaults } from '../utils/index.js'

beforeEach(async () => {
  await setupMockAgent()
  getMockAgent().enableNetConnect()
})

afterEach(async () => {
  await teardownMockAgent()
})

test('blockExoticSubdeps disallows git dependencies in subdependencies', async () => {
  prepareEmpty()

  await expect(addDependenciesToPackage({},
    // @pnpm.e2e/has-aliased-git-dependency has a git-hosted subdependency (say-hi from github:zkochan/hi)
    ['@pnpm.e2e/has-aliased-git-dependency'],
    testDefaults({ blockExoticSubdeps: true, fastUnpack: false })
  )).rejects.toThrow('is not allowed in subdependencies when blockExoticSubdeps is enabled')
})

test('blockExoticSubdeps allows git dependencies in direct dependencies', async () => {
  // Mock the HEAD request that isRepoPublic() in @pnpm/resolving.git-resolver makes to check if the repo is public.
  // Without this, transient network failures cause the resolver to fall back to git+https:// instead of
  // resolving via the codeload tarball URL.
  getMockAgent().get('https://github.com')
    .intercept({ path: '/kevva/is-negative', method: 'HEAD' })
    .reply(200)

  const project = prepareEmpty()

  // Direct git dependency should be allowed even when blockExoticSubdeps is enabled
  const { updatedManifest: manifest } = await addDependenciesToPackage(
    {},
    ['kevva/is-negative#1.0.0'],
    testDefaults({ blockExoticSubdeps: true })
  )

  project.has('is-negative')

  expect(manifest.dependencies).toStrictEqual({
    'is-negative': 'github:kevva/is-negative#1.0.0',
  })
})

test('blockExoticSubdeps allows registry dependencies in subdependencies', async () => {
  const project = prepareEmpty()

  // A package with only registry subdependencies should work fine
  await addDependenciesToPackage(
    {},
    ['is-positive@1.0.0'],
    testDefaults({ blockExoticSubdeps: true })
  )

  project.has('is-positive')
})

test('blockExoticSubdeps: false (default) allows git dependencies in subdependencies', async () => {
  const project = prepareEmpty()

  // Without blockExoticSubdeps (or with it set to false), git subdeps should be allowed
  await addDependenciesToPackage(
    {},
    ['@pnpm.e2e/has-aliased-git-dependency'],
    testDefaults({ blockExoticSubdeps: false, fastUnpack: false })
  )

  const m = project.requireModule('@pnpm.e2e/has-aliased-git-dependency')
  expect(m).toBe('Hi')
})

test('blockExoticSubdeps allows exotic dependencies in workspace packages', async () => {
  preparePackages([
    {
      location: 'project-1',
      package: { name: 'project-1' },
    },
    {
      location: 'project-2',
      package: { name: 'project-2' },
    },
  ])

  const importers: MutatedProject[] = [
    {
      mutation: 'install',
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      mutation: 'install',
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
  ]

  const allProjects = [
    {
      buildIndex: 0,
      manifest: {
        name: 'project-1',
        version: '1.0.0',
        dependencies: {
          'project-2': 'workspace:^',
        },
      },
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'project-2',
        version: '1.0.0',
        dependencies: {
          // Direct git dependency in workspace project
          'is-negative': 'github:kevva/is-negative#1.0.0',
        },
      },
      rootDir: path.resolve('project-2') as ProjectRootDir,
    },
  ]

  // Mock the HEAD request that isRepoPublic() makes to check if the repo is public.
  getMockAgent().get('https://github.com')
    .intercept({ path: '/kevva/is-negative', method: 'HEAD' })
    .reply(200)

  const workspacePackages = new Map([
    ['project-2', new Map([
      ['1.0.0', {
        rootDir: path.resolve('project-2') as ProjectRootDir,
        manifest: allProjects[1].manifest,
      }],
    ])],
  ])

  // This should not fail even when blockExoticSubdeps is true
  await mutateModules(importers, testDefaults({
    allProjects,
    blockExoticSubdeps: true,
    workspacePackages,
  }))
})
