/// <reference path="../../../__typings__/index.d.ts"/>
import { promises as fs } from 'node:fs'
import path from 'node:path'

import { describe, expect, it } from '@jest/globals'
import { createShortHash } from '@pnpm/crypto.hash'
import { getRegisteredProjects, registerProject } from '@pnpm/store.controller'
import { temporaryDirectory } from 'tempy'

import { pruneGlobalVirtualStore } from '../src/storeController/pruneGlobalVirtualStore.js'

describe('projectRegistry', () => {
  describe('registerProject()', () => {
    it('creates a symlink in the projects directory', async () => {
      const storeDir = temporaryDirectory()
      const projectDir = temporaryDirectory()

      await registerProject(storeDir, projectDir)

      // Check that projects directory was created
      const projectsDir = path.join(storeDir, 'projects')
      const entries = await fs.readdir(projectsDir)
      expect(entries).toHaveLength(1)

      // Check that the symlink points to the project
      const linkPath = path.join(projectsDir, entries[0])
      const target = await fs.readlink(linkPath)
      expect(path.resolve(path.dirname(linkPath), target)).toBe(projectDir)
    })

    it('is idempotent - registering the same project twice works', async () => {
      const storeDir = temporaryDirectory()
      const projectDir = temporaryDirectory()

      await registerProject(storeDir, projectDir)
      await registerProject(storeDir, projectDir)

      const projectsDir = path.join(storeDir, 'projects')
      const entries = await fs.readdir(projectsDir)
      expect(entries).toHaveLength(1)
    })

    it('registers multiple projects with different hashes', async () => {
      const storeDir = temporaryDirectory()
      const projectDir1 = temporaryDirectory()
      const projectDir2 = temporaryDirectory()

      await registerProject(storeDir, projectDir1)
      await registerProject(storeDir, projectDir2)

      const projectsDir = path.join(storeDir, 'projects')
      const entries = await fs.readdir(projectsDir)
      expect(entries).toHaveLength(2)
    })

    it('does not create symlink when store is inside project directory', async () => {
      const projectDir = temporaryDirectory()
      const storeDir = path.join(projectDir, 'node_modules', '.pnpm-store')
      await fs.mkdir(storeDir, { recursive: true })

      await registerProject(storeDir, projectDir)

      // The projects directory should not be created since we skipped registration
      const projectsDir = path.join(storeDir, 'projects')
      await expect(fs.readdir(projectsDir)).rejects.toThrow(/ENOENT/)
    })
  })

  describe('getRegisteredProjects()', () => {
    it('returns empty array when no projects are registered', async () => {
      const storeDir = temporaryDirectory()

      const projects = await getRegisteredProjects(storeDir)
      expect(projects).toEqual([])
    })

    it('returns registered project paths', async () => {
      const storeDir = temporaryDirectory()
      const projectDir1 = temporaryDirectory()
      const projectDir2 = temporaryDirectory()

      await registerProject(storeDir, projectDir1)
      await registerProject(storeDir, projectDir2)

      const projects = await getRegisteredProjects(storeDir)
      expect(projects.sort()).toEqual([projectDir1, projectDir2].sort())
    })

    it('cleans up stale entries for deleted projects', async () => {
      const storeDir = temporaryDirectory()
      const projectDir = temporaryDirectory()

      await registerProject(storeDir, projectDir)

      // Verify project is registered
      let projects = await getRegisteredProjects(storeDir)
      expect(projects).toEqual([projectDir])

      // Delete the project directory
      await fs.rm(projectDir, { recursive: true })

      // getRegisteredProjects should clean up stale entry
      projects = await getRegisteredProjects(storeDir)
      expect(projects).toEqual([])

      // Verify the symlink was removed
      const projectsDir = path.join(storeDir, 'projects')
      const entries = await fs.readdir(projectsDir)
      expect(entries).toEqual([])
    })

    it('handles mix of valid and stale entries', async () => {
      const storeDir = temporaryDirectory()
      const validProject = temporaryDirectory()
      const staleProject = temporaryDirectory()

      await registerProject(storeDir, validProject)
      await registerProject(storeDir, staleProject)

      // Delete one project
      await fs.rm(staleProject, { recursive: true })

      const projects = await getRegisteredProjects(storeDir)
      expect(projects).toEqual([validProject])
    })
  })

  describe('a store spelled through a symlink of another depth', () => {
    it('keeps the registration listed', async () => {
      const { root, canonicalStore, storeThroughSymlink } = await createSymlinkedStore()
      const projectDir = await createProject(root, 'project')

      await registerProject(storeThroughSymlink, projectDir)

      expect(await getRegisteredProjects(canonicalStore)).toEqual([projectDir])
      expect(await getRegisteredProjects(storeThroughSymlink)).toEqual([projectDir])
    })

    // A junction holds only an absolute path.
    const itWithRelativeLinks = process.platform === 'win32' ? it.skip : it
    itWithRelativeLinks('replaces a relative link that crosses the symlink', async () => {
      const { root, storeThroughSymlink } = await createSymlinkedStore()
      const projectDir = await createProject(root, 'project')
      const registryDir = path.join(storeThroughSymlink, 'projects')
      await fs.mkdir(registryDir)
      const linkPath = path.join(registryDir, createShortHash(projectDir))
      await fs.symlink(path.relative(registryDir, projectDir), linkPath)
      await expect(fs.stat(linkPath)).rejects.toThrow(/ENOENT/)

      await registerProject(storeThroughSymlink, projectDir)

      expect(await fs.realpath(linkPath)).toBe(await fs.realpath(projectDir))
    })

    it('lets prune keep the packages of a project registered through the symlink', async () => {
      const { root, canonicalStore, storeThroughSymlink } = await createSymlinkedStore()
      const packageDirs = await Promise.all(([
        ['direct', canonicalStore],
        ['through-symlink', storeThroughSymlink],
      ] as const).map(async ([name, storeDir]) => {
        const packageDir = path.join(canonicalStore, 'links', '@', name, '1.0.0', 'hash', 'node_modules', name)
        await fs.mkdir(packageDir, { recursive: true })
        const projectDir = await createProject(root, name)
        await fs.mkdir(path.join(projectDir, 'node_modules'))
        await fs.symlink(packageDir, path.join(projectDir, 'node_modules', name), 'junction')
        await registerProject(storeDir, projectDir)
        return packageDir
      }))

      await pruneGlobalVirtualStore(canonicalStore)

      await Promise.all(packageDirs.map(async (packageDir) => {
        expect((await fs.stat(packageDir)).isDirectory()).toBe(true)
      }))
    })
  })
})

interface SymlinkedStore {
  root: string
  canonicalStore: string
  storeThroughSymlink: string
}

async function createSymlinkedStore (): Promise<SymlinkedStore> {
  const root = await fs.realpath(temporaryDirectory())
  const canonicalStore = path.join(root, 'deep', 'nested', 'store')
  await fs.mkdir(canonicalStore, { recursive: true })
  const storeThroughSymlink = path.join(root, 'store-link')
  await fs.symlink(canonicalStore, storeThroughSymlink, 'junction')
  return { root, canonicalStore, storeThroughSymlink }
}

async function createProject (root: string, name: string): Promise<string> {
  const projectDir = path.join(root, name)
  await fs.mkdir(projectDir)
  return projectDir
}
