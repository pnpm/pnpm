/// <reference path="../../../__typings__/index.d.ts"/>
import { promises as fs } from 'fs'
import path from 'path'
import { registerProject, getRegisteredProjects } from '@pnpm/package-store'
import { temporaryDirectory } from 'tempy'

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
})
