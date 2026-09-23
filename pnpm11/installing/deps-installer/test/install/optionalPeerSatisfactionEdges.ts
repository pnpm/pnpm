import fs from 'node:fs'
import path from 'node:path'

import { expect, test } from '@jest/globals'
import {
  install,
  mutateModules,
  mutateModulesInSingleProject,
  type ProjectOptions,
} from '@pnpm/installing.deps-installer'
import { prepareEmpty, preparePackages } from '@pnpm/prepare'
import type { ProjectManifest, ProjectRootDir } from '@pnpm/types'

import { testDefaults } from '../utils/index.js'

const PROD_ONLY = {
  dependencies: true,
  devDependencies: false,
  optionalDependencies: true,
}

const ABC = '@pnpm.e2e/abc-optional-peers'
const PEER_A = '@pnpm.e2e/peer-a'
const PEER_C = '@pnpm.e2e/peer-c'

const MANIFEST: ProjectManifest = {
  dependencies: { [ABC]: '1.0.0' },
  devDependencies: { [PEER_A]: '1.0.0', [PEER_C]: '1.0.0' },
}

interface InstallOpts {
  frozenLockfile?: boolean
  include?: typeof PROD_ONLY
  lockfileOnly?: boolean
  nodeLinker?: 'hoisted'
  resolvePeersFromWorkspaceRoot?: boolean
}

async function installInSingleProject (manifest: ProjectManifest, opts: InstallOpts): Promise<void> {
  await mutateModulesInSingleProject({
    manifest,
    mutation: 'install',
    rootDir: process.cwd() as ProjectRootDir,
  }, testDefaults(opts))
}

test('a production install leaves out a devDependency that only satisfies an optional peer of a production dependency', async () => {
  const project = prepareEmpty()
  await installInSingleProject(MANIFEST, { lockfileOnly: true })

  await installInSingleProject(MANIFEST, { frozenLockfile: true, include: PROD_ONLY })

  expect(virtualStoreEntries(PEER_C)).toHaveLength(0)
  expect(virtualStoreEntries(PEER_A)).toHaveLength(1)
  const abcModulesDir = abcNodeModulesDir()
  expect(fs.existsSync(path.join(abcModulesDir, PEER_C))).toBe(false)
  // A required peer is installed even when only a devDependency provides it.
  expect(fs.existsSync(path.join(abcModulesDir, PEER_A))).toBe(true)
  const currentAbcSnapshot = abcSnapshot(project.readCurrentLockfile())
  expect(Object.keys({ ...currentAbcSnapshot.dependencies, ...currentAbcSnapshot.optionalDependencies }).sort()).toStrictEqual([PEER_A])

  await installInSingleProject(MANIFEST, { frozenLockfile: true })

  expect(fs.existsSync(path.join(abcNodeModulesDir(), PEER_C))).toBe(true)
})

test('a production install after a full install removes the link to a devDependency that only satisfies an optional peer', async () => {
  const project = prepareEmpty()
  await installInSingleProject(MANIFEST, {})
  expect(fs.existsSync(path.join(abcNodeModulesDir(), PEER_C))).toBe(true)

  await installInSingleProject(MANIFEST, { frozenLockfile: true, include: PROD_ONLY })

  expect(Object.keys(project.readCurrentLockfile().snapshots).filter((depPath) => depPath.startsWith(`${PEER_C}@`))).toHaveLength(0)
  expect(lstatOrNull(path.join(abcNodeModulesDir(), PEER_C))).toBeNull()
  expect(fs.existsSync(path.join(abcNodeModulesDir(), PEER_A))).toBe(true)
})

test('a production install that resolves the lockfile leaves out a devDependency that only satisfies an optional peer', async () => {
  const project = prepareEmpty()

  await installInSingleProject(MANIFEST, { include: PROD_ONLY })

  expect(virtualStoreEntries(PEER_C)).toHaveLength(0)
  expect(lstatOrNull(path.join(abcNodeModulesDir(), PEER_C))).toBeNull()
  expect(fs.existsSync(path.join(abcNodeModulesDir(), PEER_A))).toBe(true)
  const currentLockfile = project.readCurrentLockfile()
  const currentAbcSnapshot = abcSnapshot(currentLockfile)
  expect(Object.keys({ ...currentAbcSnapshot.dependencies, ...currentAbcSnapshot.optionalDependencies })).not.toContain(PEER_C)
  expect(Object.keys(currentLockfile.snapshots).filter((depPath) => depPath.startsWith(`${PEER_C}@`))).toHaveLength(0)
  // The wanted lockfile still records the peer resolution.
  const wantedAbcSnapshot = abcSnapshot(project.readLockfile())
  expect(Object.keys({ ...wantedAbcSnapshot.dependencies, ...wantedAbcSnapshot.optionalDependencies })).toContain(PEER_C)
})

test('a production install with the hoisted node linker leaves out a devDependency that only satisfies an optional peer', async () => {
  const project = prepareEmpty()
  await installInSingleProject(MANIFEST, { lockfileOnly: true })

  await installInSingleProject(MANIFEST, { frozenLockfile: true, include: PROD_ONLY, nodeLinker: 'hoisted' })

  project.has(ABC)
  project.has(PEER_A)
  project.hasNot(PEER_C)
})

test('a production install keeps an optional peer that an ancestor package provides', async () => {
  prepareEmpty()
  const manifest: ProjectManifest = {
    dependencies: { '@pnpm.e2e/abc-optional-peers-parent': '1.0.0' },
    devDependencies: { [PEER_C]: '1.0.0' },
  }
  await installInSingleProject(manifest, { lockfileOnly: true })

  await installInSingleProject(manifest, { frozenLockfile: true, include: PROD_ONLY })

  expect(virtualStoreEntries(PEER_C)).toHaveLength(1)
  expect(fs.existsSync(path.join(abcNodeModulesDir(), PEER_C))).toBe(true)
})

test('a production install keeps an auto-installed required peer', async () => {
  prepareEmpty()
  const manifest: ProjectManifest = {
    dependencies: { [ABC]: '1.0.0' },
    devDependencies: { '@pnpm.e2e/foo': '100.0.0' },
  }
  await install(manifest, testDefaults({ autoInstallPeers: true, lockfileOnly: true }))

  await install(manifest, testDefaults({ autoInstallPeers: true, frozenLockfile: true, include: PROD_ONLY }))

  expect(fs.existsSync(path.join(abcNodeModulesDir(), PEER_A))).toBe(true)
})

test('a production install classifies an optional peer that the workspace root lists by resolvePeersFromWorkspaceRoot', async () => {
  preparePackages([
    {
      location: '.',
      package: { name: 'root' },
    },
    {
      location: 'project-1',
      package: { name: 'project-1' },
    },
  ])
  const allProjects: ProjectOptions[] = [
    {
      buildIndex: 0,
      manifest: {
        name: 'root',
        version: '1.0.0',
        devDependencies: { [PEER_A]: '1.0.0', [PEER_C]: '1.0.0' },
      },
      rootDir: process.cwd() as ProjectRootDir,
    },
    {
      buildIndex: 0,
      manifest: {
        name: 'project-1',
        version: '1.0.0',
        dependencies: { [ABC]: '1.0.0' },
      },
      rootDir: path.resolve('project-1') as ProjectRootDir,
    },
  ]
  const installAll = async (opts: InstallOpts) => mutateModules(
    allProjects.map(({ rootDir }) => ({ mutation: 'install', rootDir })),
    testDefaults({ allProjects, ...opts })
  )
  await installAll({ lockfileOnly: true, resolvePeersFromWorkspaceRoot: true })

  await installAll({ frozenLockfile: true, include: PROD_ONLY, resolvePeersFromWorkspaceRoot: false })
  expect(virtualStoreEntries(PEER_C)).toHaveLength(1)
  expect(fs.existsSync(path.join(abcNodeModulesDir(), PEER_C))).toBe(true)

  await installAll({ frozenLockfile: true, include: PROD_ONLY, resolvePeersFromWorkspaceRoot: true })
  expect(lstatOrNull(path.join(abcNodeModulesDir(), PEER_C))).toBeNull()
  expect(fs.existsSync(path.join(abcNodeModulesDir(), PEER_A))).toBe(true)
})

function virtualStoreEntries (pkgName: string): string[] {
  const prefix = `${pkgName.replace('/', '+')}@`
  return fs.readdirSync('node_modules/.pnpm').filter((dir) => dir.startsWith(prefix))
}

function abcNodeModulesDir (): string {
  const [dir] = virtualStoreEntries(ABC)
  return path.resolve('node_modules/.pnpm', dir, 'node_modules')
}

function abcSnapshot (lockfile: { snapshots: Record<string, { dependencies?: Record<string, string>, optionalDependencies?: Record<string, string> }> }) {
  const depPaths = Object.keys(lockfile.snapshots).filter((depPath) => depPath.startsWith(`${ABC}@`))
  expect(depPaths).toHaveLength(1)
  return lockfile.snapshots[depPaths[0]]
}

function lstatOrNull (filePath: string): fs.Stats | null {
  try {
    return fs.lstatSync(filePath)
  } catch (err: unknown) {
    if ((err as NodeJS.ErrnoException).code === 'ENOENT') return null
    throw err
  }
}
