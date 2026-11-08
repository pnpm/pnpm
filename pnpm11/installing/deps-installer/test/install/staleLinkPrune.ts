import fs from 'node:fs'
import path from 'node:path'

import { expect, jest, test } from '@jest/globals'
import { type MutatedProject, mutateModules, type ProjectOptions } from '@pnpm/installing.deps-installer'
import { preparePackages } from '@pnpm/prepare'
import type { ProjectManifest, ProjectRootDir } from '@pnpm/types'

import { testDefaults } from '../utils/index.js'

const LINKERS = ['isolated', 'hoisted'] as const

interface Workspace {
  importers: MutatedProject[]
  allProjects: ProjectOptions[]
}

function prepareWorkspace (manifests: Record<string, ProjectManifest>): Workspace {
  preparePackages(Object.entries(manifests).map(([location, manifest]) => ({ location, package: manifest })))
  const allProjects = Object.entries(manifests).map(([location, manifest]) => ({
    buildIndex: 0,
    manifest,
    rootDir: path.resolve(location) as ProjectRootDir,
  }))
  return {
    allProjects,
    importers: allProjects.map(({ rootDir }) => ({ mutation: 'install', rootDir })),
  }
}

function writeManifest (dir: string, manifest: ProjectManifest): void {
  fs.mkdirSync(dir, { recursive: true })
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify(manifest))
}

/**
 * The distinct `project dir:alias@version` of the `pnpm:root` `removed` events. The
 * hoisted install hands `reporter` to the headless install, which attaches it
 * a second time, so each event of that phase reaches it twice.
 */
function removals (reporter: jest.Mock): string[] {
  return Array.from(new Set(reporter.mock.calls
    .map(([log]) => log as { name: string, prefix: string, removed?: { name: string, version?: string } })
    .filter((log) => log.name === 'pnpm:root' && log.removed != null)
    .map((log) => `${path.relative(process.cwd(), log.prefix)}:${log.removed!.name}@${log.removed!.version}`)))
}

function isSymlink (linkPath: string): boolean {
  return fs.lstatSync(linkPath, { throwIfNoEntry: false })?.isSymbolicLink() === true
}

test.each(LINKERS)('a removed link: dependency is reported (nodeLinker: %s)', async (nodeLinker) => {
  const { allProjects, importers } = prepareWorkspace({
    '': { name: 'root', dependencies: { lib: 'link:lib' } },
  })
  writeManifest('lib', { name: 'lib', version: '1.0.0' })
  await mutateModules(importers, testDefaults({ allProjects, nodeLinker }))
  expect(isSymlink('node_modules/lib')).toBe(true)

  allProjects[0].manifest.dependencies = {}
  const reporter = jest.fn()
  await mutateModules(importers, testDefaults({ allProjects, nodeLinker, reporter }))

  expect(fs.existsSync('node_modules/lib')).toBe(false)
  expect(removals(reporter)).toStrictEqual([':lib@1.0.0'])
})

// With a bin, the link leaves a real `.bin`, so the removal reads the manifest
// to remove its bins.
test.each(LINKERS.flatMap((nodeLinker) => [
  { nodeLinker, withBin: false },
  { nodeLinker, withBin: true },
]))('a removed link: dependency with a malformed manifest is unlinked (nodeLinker: $nodeLinker, withBin: $withBin)', async ({ nodeLinker, withBin }) => {
  const { allProjects, importers } = prepareWorkspace({
    '': { name: 'root', dependencies: { lib: 'link:lib' } },
  })
  writeManifest('lib', { name: 'lib', version: '1.0.0', ...(withBin ? { bin: 'cli.js' } : {}) })
  fs.writeFileSync('lib/cli.js', '#!/usr/bin/env node\n')
  await mutateModules(importers, testDefaults({ allProjects, nodeLinker }))
  expect(fs.existsSync('node_modules/.bin')).toBe(withBin)
  fs.writeFileSync('lib/package.json', '{')

  allProjects[0].manifest.dependencies = {}
  const reporter = jest.fn()
  await mutateModules(importers, testDefaults({ allProjects, nodeLinker, reporter }))

  expect(fs.lstatSync('node_modules/lib', { throwIfNoEntry: false })).toBeUndefined()
  expect(removals(reporter)).toStrictEqual([':lib@undefined'])
})

test.each(LINKERS)('a project link the root starts providing under another spelling is removed (nodeLinker: %s)', async (nodeLinker) => {
  const { allProjects, importers } = prepareWorkspace({
    '': { name: 'root' },
    'packages/app': { name: 'app', dependencies: { shared: 'link:../shared' } },
    'packages/shared': { name: 'shared', version: '1.0.0' },
  })
  await mutateModules(importers, testDefaults({ allProjects, dedupeDirectDeps: true, nodeLinker }))
  expect(isSymlink('packages/app/node_modules/shared')).toBe(true)

  allProjects[0].manifest.dependencies = { shared: 'link:packages/shared' }
  const reporter = jest.fn()
  await mutateModules(importers, testDefaults({ allProjects, dedupeDirectDeps: true, nodeLinker, reporter }))

  expect(isSymlink('node_modules/shared')).toBe(true)
  expect(fs.lstatSync('packages/app/node_modules/shared', { throwIfNoEntry: false })).toBeUndefined()
  expect(removals(reporter)).toStrictEqual([`${path.join('packages', 'app')}:shared@1.0.0`])
})

// The lockfile still records the project's deduped link, so each later install
// finds it stale again, with nothing left to unlink.
test.each(LINKERS)('a deduped link is reported once (nodeLinker: %s)', async (nodeLinker) => {
  const { allProjects, importers } = prepareWorkspace({
    '': { name: 'root' },
    'packages/app': { name: 'app', dependencies: { shared: 'link:../shared' } },
    'packages/shared': { name: 'shared' },
  })
  writeManifest('other', { name: 'other' })
  await mutateModules(importers, testDefaults({ allProjects, dedupeDirectDeps: true, nodeLinker }))
  allProjects[0].manifest.dependencies = { shared: 'link:packages/shared' }
  await mutateModules(importers, testDefaults({ allProjects, dedupeDirectDeps: true, nodeLinker }))

  allProjects[0].manifest.dependencies = { shared: 'link:packages/shared', other: 'link:other' }
  const reporter = jest.fn()
  await mutateModules(importers, testDefaults({ allProjects, dedupeDirectDeps: true, nodeLinker, reporter }))

  expect(removals(reporter)).toStrictEqual([])
})

test('a filtered install that leaves out the root keeps a project link the root provides (nodeLinker: hoisted)', async () => {
  const nodeLinker = 'hoisted'
  const { allProjects, importers } = prepareWorkspace({
    '': { name: 'root', dependencies: { shared: 'link:packages/shared' } },
    'packages/app': { name: 'app', dependencies: { shared: 'link:../shared' } },
    'packages/shared': { name: 'shared' },
  })
  await mutateModules(importers, testDefaults({ allProjects, dedupeDirectDeps: false, nodeLinker }))
  expect(isSymlink('packages/app/node_modules/shared')).toBe(true)

  const reporter = jest.fn()
  await mutateModules([importers[1]], testDefaults({ allProjects, dedupeDirectDeps: true, nodeLinker, reporter }))

  expect(isSymlink('packages/app/node_modules/shared')).toBe(true)
  expect(removals(reporter)).toStrictEqual([])
})

test.each(LINKERS)('a project link spelled like the root\'s to another directory is kept (nodeLinker: %s)', async (nodeLinker) => {
  const { allProjects, importers } = prepareWorkspace({
    '': { name: 'root', dependencies: { x: 'link:vendor/x' } },
    'packages/app': { name: 'app', dependencies: { x: 'link:vendor/x' } },
  })
  writeManifest('vendor/x', { name: 'root-x' })
  writeManifest('packages/app/vendor/x', { name: 'app-x' })
  writeManifest('other', { name: 'other' })
  await mutateModules(importers, testDefaults({ allProjects, dedupeDirectDeps: true, nodeLinker }))

  // A second install with a change to make, so it runs the prune.
  allProjects[0].manifest.dependencies = { x: 'link:vendor/x', other: 'link:other' }
  const reporter = jest.fn()
  await mutateModules(importers, testDefaults({ allProjects, dedupeDirectDeps: true, nodeLinker, reporter }))

  const linkedManifest = JSON.parse(fs.readFileSync('packages/app/node_modules/x/package.json', 'utf8'))
  expect(linkedManifest.name).toBe('app-x')
  expect(removals(reporter)).toStrictEqual([])
})
