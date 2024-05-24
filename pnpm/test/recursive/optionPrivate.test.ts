import { sync as writeYamlFile } from 'write-yaml-file'
import { execPnpm } from '../utils'
import { preparePackages } from '@pnpm/prepare'
import { createTestIpcServer } from '@pnpm/test-ipc-server'
import { type ProjectManifest } from '@pnpm/types'

describe('--private / --no-private options', () => {
  test('Omitting --private / --no-private includes all packages', async () => {
    const packages = [
      { name: 'app', private: true, dependencies: ['open-source-library-a', 'library-d'], devDependencies: ['build-tool-b'] },
      { name: 'open-source-library-a', dependencies: ['open-source-library-c'], devDependencies: ['build-tool-a'] },
      { name: 'open-source-library-b', devDependencies: ['build-tool-a'] },
      { name: 'open-source-library-c', devDependencies: ['build-tool-a'] },
      { name: 'library-d', private: true, devDependencies: ['build-tool-a'] },
      { name: 'build-tool-a', private: true },
      { name: 'build-tool-b' },
    ]
    const allPackages = packages.map(({ name }) => name).sort()
    await using server = await setup(packages)

    await execPnpm(['recursive', 'test', '-r'])

    expect(server.getLines().sort()).toEqual(allPackages)
  })

  describe('--private', () => {
    test('includes only "private: true" packages', async () => {
      await using server = await setup([
        { name: 'app', private: true, dependencies: ['open-source-library-a', 'library-d'], devDependencies: ['build-tool-b'] },
        { name: 'open-source-library-a', dependencies: ['open-source-library-c'], devDependencies: ['build-tool-a'] },
        { name: 'open-source-library-b', devDependencies: ['build-tool-a'] },
        { name: 'open-source-library-c', devDependencies: ['build-tool-a'] },
        { name: 'library-d', private: true, devDependencies: ['build-tool-a'] },
        { name: 'build-tool-a', private: true },
        { name: 'build-tool-b' },
      ])

      await execPnpm(['recursive', 'test', '-r', '--private'])

      expect(server.getLines().sort()).toEqual(['app', 'build-tool-a', 'library-d'])
    })

    test('includes private dependencies of public packages', async () => {
      await using server = await setup([
        { name: 'open-source-library-a', devDependencies: ['build-tool-a'] },
        { name: 'build-tool-a', private: true },
      ])

      await execPnpm(['recursive', 'test', '--filter=open-source-library-a...', '--private'])

      expect(server.getLines().sort()).toEqual(['build-tool-a'])
    })

    test('omits "private: false" packages', async () => {
      await using server = await setup([
        { name: 'app', private: true, dependencies: ['open-source-library-a'], devDependencies: ['build-tool-b'] },
        { name: 'open-source-library-a', private: false, dependencies: ['open-source-library-c'], devDependencies: ['build-tool-a'] },
        { name: 'open-source-library-b', private: false, devDependencies: ['build-tool-a'] },
        { name: 'open-source-library-c', private: false, devDependencies: ['build-tool-a'] },
        { name: 'build-tool-a', private: true },
        { name: 'build-tool-b', private: false },
      ])

      await execPnpm(['recursive', 'test', '-r', '--private'])

      const outPackages = server.getLines().sort()
      for (const pkg of ['open-source-library-a', 'open-source-library-b', 'open-source-library-c', 'build-tool-b']) {
        expect(outPackages).not.toContain(pkg)
      }
    })
  })

  describe('--no-private', () => {
    test('omits any "private: true" packages', async () => {
      await using server = await setup([
        { name: 'open-source-library-a', dependencies: ['open-source-library-c'], devDependencies: ['build-tool'] },
        { name: 'open-source-library-b', devDependencies: ['build-tool'] },
        { name: 'open-source-library-c', devDependencies: ['build-tool'] },
        { name: 'build-tool', private: true },
      ])

      await execPnpm(['recursive', 'test', '-r', '--no-private'])

      const outPackages = server.getLines().sort()
      expect(outPackages).not.toContain('build-tool')
      expect(outPackages).toEqual(['open-source-library-a', 'open-source-library-b', 'open-source-library-c'])
    })

    test('includes "private: false" packages', async () => {
      await using server = await setup([
        { name: 'app', private: true, dependencies: ['open-source-library-a'], devDependencies: ['build-tool-b'] },
        { name: 'open-source-library-a', private: false, dependencies: ['open-source-library-c'], devDependencies: ['build-tool-a'] },
        { name: 'open-source-library-b', private: false, devDependencies: ['build-tool-a'] },
        { name: 'open-source-library-c', private: false, devDependencies: ['build-tool-a'] },
        { name: 'build-tool-a', private: true },
        { name: 'build-tool-b', private: false },
      ])

      await execPnpm(['recursive', 'test', '-r', '--no-private'])

      const outPackages = server.getLines().sort()
      for (const pkg of ['open-source-library-a', 'open-source-library-b', 'open-source-library-c', 'build-tool-b']) {
        expect(outPackages).toContain(pkg)
      }
    })

    test('includes public dependencies of private packages', async () => {
      await using server = await setup([
        { name: 'app', private: true, dependencies: ['open-source-library-a'], devDependencies: ['build-tool-b'] },
        { name: 'open-source-library-a', dependencies: ['open-source-library-c'], devDependencies: ['build-tool-a'] },
        { name: 'open-source-library-b', devDependencies: ['build-tool-a'] },
        { name: 'open-source-library-c', devDependencies: ['build-tool-a'] },
        { name: 'build-tool-a', private: true },
        { name: 'build-tool-b' },
      ])

      await execPnpm(['recursive', 'test', '--filter=app...', '--no-private'])

      const outPackages = server.getLines().sort()
      expect(outPackages).not.toContain('app')
      expect(outPackages).not.toContain('build-tool-a')
      expect(outPackages).toEqual(['build-tool-b', 'open-source-library-a', 'open-source-library-c'])
    })
  })
})

// Helper functions

interface BaseProject {
  name: string
  private?: boolean
  dependencies?: string[]
  devDependencies?: string[]
}

const setup = async (workspacePackages: BaseProject[]) => {
  const server = await createTestIpcServer()

  const projects: ProjectManifest[] = workspacePackages.map((project) => ({
    name: project.name,
    ...(project.private !== undefined ? { private: project.private } : {}),
    version: '1.0.0',
    dependencies: makeDependencies(project.dependencies),
    devDependencies: makeDependencies(project.devDependencies),
    scripts: {
      test: server.sendLineScript(project.name),
    },
  }))
  preparePackages(projects)

  writeYamlFile('pnpm-workspace.yaml', { packages: ['**', '!store/**'] })
  await execPnpm(['install'])

  return server
}

const makeDependencies = (deps: string[] = []) => Object.fromEntries(deps.map((dep) => [dep, 'workspace:*']))
