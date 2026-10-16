import fs from 'node:fs'
import path from 'node:path'

import { beforeEach, describe, expect, test } from '@jest/globals'
import { pkg } from '@pnpm/pkg-manifest.commands'
import { tempDir } from '@pnpm/prepare'

const { cliOptionsTypes, handler } = pkg

describe('pkg command', () => {
  let tmpDir: string

  beforeEach(() => {
    tmpDir = tempDir()
  })

  describe('get subcommand', () => {
    test('gets all fields when no keys provided', async () => {
      const manifest = {
        name: 'test-package',
        version: '1.0.0',
        dependencies: { foo: '1.0.0' },
      }
      fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

      const result = await handler({ dir: tmpDir }, ['get'])
      const parsed = JSON.parse(result as string)
      expect(parsed.name).toBe('test-package')
      expect(parsed.version).toBe('1.0.0')
    })

    test('gets a single key as a raw value', async () => {
      const manifest = { name: 'test-package', version: '1.0.0' }
      fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

      expect(await handler({ dir: tmpDir }, ['get', 'name'])).toBe('test-package')
    })

    test('returns an empty string when a single key is missing', async () => {
      const manifest = { name: 'test-package' }
      fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

      expect(await handler({ dir: tmpDir }, ['get', 'description'])).toBe('')
    })

    test('gets a single key as JSON when --json is set', async () => {
      const manifest = { name: 'test-package' }
      fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

      expect(await handler({ dir: tmpDir, json: true }, ['get', 'name'])).toBe('"test-package"')
    })

    test('returns an object when multiple keys are requested', async () => {
      const manifest = {
        name: 'test-package',
        version: '1.0.0',
        description: 'A test package',
      }
      fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

      const parsed = JSON.parse(await handler({ dir: tmpDir }, ['get', 'name', 'version']) as string)
      expect(parsed.name).toBe('test-package')
      expect(parsed.version).toBe('1.0.0')
      expect(parsed.description).toBeUndefined()
    })

    test('gets nested keys using dot notation', async () => {
      const manifest = {
        name: 'test-package',
        scripts: { build: 'tsc', test: 'jest' },
      }
      fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

      expect(await handler({ dir: tmpDir }, ['get', 'scripts.build'])).toBe('tsc')
    })

    test('gets a hyphenated key using dot notation', async () => {
      const manifest = {
        name: 'test-package',
        dependencies: { 'some-package-name': '0.23.4' },
      }
      fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

      expect(await handler({ dir: tmpDir }, ['get', 'dependencies.some-package-name'])).toBe('0.23.4')
    })
  })

  describe('set subcommand', () => {
    test('sets a simple key-value pair', async () => {
      const manifest = { name: 'test-package' }
      fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

      await handler({ dir: tmpDir }, ['set', 'version=1.0.0'])
      const updated = JSON.parse(fs.readFileSync(path.join(tmpDir, 'package.json'), 'utf8'))
      expect(updated.version).toBe('1.0.0')
    })

    test('sets nested keys using dot notation', async () => {
      const manifest = { name: 'test-package' }
      fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

      await handler({ dir: tmpDir }, ['set', 'scripts.build=tsc'])
      const updated = JSON.parse(fs.readFileSync(path.join(tmpDir, 'package.json'), 'utf8'))
      expect(updated.scripts.build).toBe('tsc')
    })

    test('sets a hyphenated key using dot notation', async () => {
      const manifest = { name: 'test-package' }
      fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

      await handler({ dir: tmpDir }, ['set', 'dependencies.some-package-name=0.23.4'])
      const updated = JSON.parse(fs.readFileSync(path.join(tmpDir, 'package.json'), 'utf8'))
      expect(updated.dependencies).toStrictEqual({ 'some-package-name': '0.23.4' })
    })

    test('sets multiple key-value pairs', async () => {
      const manifest = { name: 'test-package' }
      fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

      await handler({ dir: tmpDir }, ['set', 'version=1.0.0', 'description=A test'])
      const updated = JSON.parse(fs.readFileSync(path.join(tmpDir, 'package.json'), 'utf8'))
      expect(updated.version).toBe('1.0.0')
      expect(updated.description).toBe('A test')
    })

    test('sets JSON values with --json flag', async () => {
      const manifest = { name: 'test-package' }
      fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

      await handler({ dir: tmpDir, json: true }, ['set', 'version=2'])
      const updated = JSON.parse(fs.readFileSync(path.join(tmpDir, 'package.json'), 'utf8'))
      expect(updated.version).toBe(2)
    })

    test('throws error for invalid key=value format', async () => {
      const manifest = { name: 'test-package' }
      fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

      await expect(handler({ dir: tmpDir }, ['set', 'invalidformat'])).rejects.toThrow()
    })

    test('sets nested values through array index notation', async () => {
      const manifest = { name: 'test-package' }
      fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

      await handler({ dir: tmpDir }, ['set', 'contributors[0].name=Alice'])
      const updated = JSON.parse(fs.readFileSync(path.join(tmpDir, 'package.json'), 'utf8'))
      expect(updated.contributors).toEqual([{ name: 'Alice' }])
    })

    test('replaces a scalar intermediate value with an object when descending', async () => {
      const manifest = { scripts: 'echo hi' }
      fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

      await handler({ dir: tmpDir }, ['set', 'scripts.test=vitest'])
      const updated = JSON.parse(fs.readFileSync(path.join(tmpDir, 'package.json'), 'utf8'))
      expect(updated.scripts).toEqual({ test: 'vitest' })
    })

    test('rejects unsafe keys to prevent prototype pollution', async () => {
      const manifest = { name: 'test-package' }
      fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

      await expect(handler({ dir: tmpDir }, ['set', '__proto__.polluted=true'])).rejects.toThrow()
      await expect(handler({ dir: tmpDir }, ['set', 'constructor.prototype.polluted=true'])).rejects.toThrow()
      expect(({} as Record<string, unknown>).polluted).toBeUndefined()
    })
  })

  describe('delete subcommand', () => {
    test('deletes a key from package.json', async () => {
      const manifest = {
        name: 'test-package',
        version: '1.0.0',
        description: 'To be deleted',
      }
      fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

      await handler({ dir: tmpDir }, ['delete', 'description'])
      const updated = JSON.parse(fs.readFileSync(path.join(tmpDir, 'package.json'), 'utf8'))
      expect(updated.description).toBeUndefined()
      expect(updated.name).toBe('test-package')
    })

    test('deletes nested keys using dot notation', async () => {
      const manifest = {
        name: 'test-package',
        scripts: { build: 'tsc', test: 'jest' },
      }
      fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

      await handler({ dir: tmpDir }, ['delete', 'scripts.test'])
      const updated = JSON.parse(fs.readFileSync(path.join(tmpDir, 'package.json'), 'utf8'))
      expect(updated.scripts.test).toBeUndefined()
      expect(updated.scripts.build).toBe('tsc')
    })

    test('throws error when no keys provided', async () => {
      const manifest = { name: 'test-package' }
      fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

      await expect(handler({ dir: tmpDir }, ['delete'])).rejects.toThrow()
    })

    test('removes an array element without leaving a hole', async () => {
      const manifest = {
        name: 'test-package',
        contributors: [{ name: 'Alice' }, { name: 'Bob' }],
      }
      fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

      await handler({ dir: tmpDir }, ['delete', 'contributors[0]'])
      const updated = JSON.parse(fs.readFileSync(path.join(tmpDir, 'package.json'), 'utf8'))
      expect(updated.contributors).toEqual([{ name: 'Bob' }])
    })
  })

  describe('fix subcommand', () => {
    test('fixes invalid name field', async () => {
      const manifest = {
        name: 123 as unknown,
        version: '1.0.0',
      }
      fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

      await handler({ dir: tmpDir }, ['fix'])
      const updated = JSON.parse(fs.readFileSync(path.join(tmpDir, 'package.json'), 'utf8'))
      expect(updated.name).toBeUndefined()
      expect(updated.version).toBe('1.0.0')
    })

    test('fixes invalid dependencies field', async () => {
      const manifest = {
        name: 'test-package',
        dependencies: 'invalid' as unknown,
      }
      fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

      await handler({ dir: tmpDir }, ['fix'])
      const updated = JSON.parse(fs.readFileSync(path.join(tmpDir, 'package.json'), 'utf8'))
      expect(updated.dependencies).toBeUndefined()
    })

    test('removes array-valued object fields', async () => {
      const manifest: Record<string, unknown> = {
        name: 'test-package',
        dependencies: [],
        scripts: [],
      }
      fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

      await handler({ dir: tmpDir }, ['fix'])
      const updated = JSON.parse(fs.readFileSync(path.join(tmpDir, 'package.json'), 'utf8'))
      expect(updated.dependencies).toBeUndefined()
      expect(updated.scripts).toBeUndefined()
    })

    test('removes null-valued object fields', async () => {
      const manifest: Record<string, unknown> = {
        name: 'test-package',
        dependencies: null,
      }
      fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

      await handler({ dir: tmpDir }, ['fix'])
      const updated = JSON.parse(fs.readFileSync(path.join(tmpDir, 'package.json'), 'utf8'))
      expect(updated.dependencies).toBeUndefined()
    })

    async function fixRepository (repository: unknown): Promise<unknown> {
      fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify({ name: 'test-package', repository }, null, 2))
      await handler({ dir: tmpDir }, ['fix'])
      return JSON.parse(fs.readFileSync(path.join(tmpDir, 'package.json'), 'utf8')).repository
    }

    test.each([
      ['github:pnpm/example', 'git+https://github.com/pnpm/example.git'],
      ['pnpm/example', 'git+https://github.com/pnpm/example.git'],
      ['https://github.com/pnpm/example', 'git+https://github.com/pnpm/example.git'],
      ['https://github.com/pnpm/example.git', 'git+https://github.com/pnpm/example.git'],
      ['https://www.github.com/pnpm/example', 'git+https://github.com/pnpm/example.git'],
      ['git+https://github.com/pnpm/example.git', 'git+https://github.com/pnpm/example.git'],
      ['git@github.com:pnpm/example.git', 'git+ssh://git@github.com/pnpm/example.git'],
      ['ssh://git@github.com/pnpm/example.git', 'git+ssh://git@github.com/pnpm/example.git'],
      ['git+ssh://git@github.com/pnpm/example.git', 'git+ssh://git@github.com/pnpm/example.git'],
      ['http://github.com/pnpm/example', 'git+ssh://git@github.com/pnpm/example.git'],
      ['git://github.com/pnpm/example.git', 'git://github.com/pnpm/example.git'],
      ['gitlab:foo/bar', 'git+https://gitlab.com/foo/bar.git'],
      ['https://gitlab.com/group/sub/proj', 'git+https://gitlab.com/group/sub/proj.git'],
      ['git://gitlab.com/foo/bar.git', 'git://gitlab.com/foo/bar.git'],
      ['bitbucket:foo/bar', 'git+https://bitbucket.org/foo/bar.git'],
      ['gist:abc123', 'git+https://gist.github.com/abc123.git'],
      ['github:pnpm/example#v1', 'git+https://github.com/pnpm/example.git#v1'],
      ['github:pnpm/example#semver:^1', 'git+https://github.com/pnpm/example.git#semver:^1'],
      ['https://example.com/repo.git', 'https://example.com/repo.git'],
    ])('converts the repository string %p to an object with url %p', async (repository, url) => {
      const fixed = await fixRepository(repository)
      expect(fixed).toStrictEqual({ type: 'git', url })
      expect(Object.keys(fixed as object)).toStrictEqual(['type', 'url'])
    })

    test('leaves the url of a repository shortcut with no owner as written', async () => {
      expect(await fixRepository('github:skills')).toStrictEqual({ type: 'git', url: 'github:skills' })
    })

    test('normalizes the url of a repository object and keeps its other fields', async () => {
      expect(await fixRepository({ type: 'git', url: 'github:pnpm/example', directory: 'packages/x' })).toStrictEqual({
        type: 'git',
        url: 'git+https://github.com/pnpm/example.git',
        directory: 'packages/x',
      })
    })

    test('does not add a type to a repository object without one', async () => {
      expect(await fixRepository({ url: 'https://github.com/pnpm/example' })).toStrictEqual({
        url: 'git+https://github.com/pnpm/example.git',
      })
    })

    test.each([
      [''],
      [{ type: 'git', url: 42 }],
      [['github:pnpm/example']],
      [true],
    ])('leaves the repository %p unchanged', async (repository) => {
      expect(await fixRepository(repository)).toStrictEqual(repository)
    })
  })

  describe('error handling', () => {
    test('throws error for unknown subcommand', async () => {
      const manifest = { name: 'test-package' }
      fs.writeFileSync(path.join(tmpDir, 'package.json'), JSON.stringify(manifest, null, 2))

      await expect(handler({ dir: tmpDir }, ['unknown'])).rejects.toThrow()
    })

    test('throws error when no subcommand provided', async () => {
      await expect(handler({ dir: tmpDir }, [])).rejects.toThrow()
    })
  })

  describe('cliOptionsTypes', () => {
    test('returns correct option types', () => {
      const types = cliOptionsTypes()
      expect(types).toHaveProperty('dir')
      expect(types).toHaveProperty('json')
      expect(types).toHaveProperty('recursive')
      expect(types).not.toHaveProperty('workspace')
      expect(types).not.toHaveProperty('workspaces')
      expect(types).not.toHaveProperty('ws')
    })
  })

  describe('recursive mode', () => {
    function setupWorkspace (manifests: Record<string, Record<string, unknown>>) {
      const allProjects = Object.entries(manifests).map(([name, manifest]) => {
        const rootDir = path.join(tmpDir, name)
        fs.mkdirSync(rootDir, { recursive: true })
        fs.writeFileSync(path.join(rootDir, 'package.json'), JSON.stringify(manifest, null, 2))
        return { rootDir, manifest }
      })
      const selectedProjectsGraph = Object.fromEntries(
        allProjects.map((project) => [project.rootDir, { package: project }])
      )
      return { allProjects, selectedProjectsGraph }
    }

    test('aggregates `get` results from each selected workspace package', async () => {
      const { selectedProjectsGraph } = setupWorkspace({
        'pkg-a': { name: 'pkg-a', version: '1.0.0' },
        'pkg-b': { name: 'pkg-b', version: '2.0.0' },
      })

      const result = await handler({
        dir: tmpDir,
        workspaceDir: tmpDir,
        recursive: true,
        selectedProjectsGraph,
      }, ['get', 'name'])

      expect(JSON.parse(result as string)).toEqual({
        'pkg-a': { name: 'pkg-a' },
        'pkg-b': { name: 'pkg-b' },
      })
    })

    test('keys projects that share a name by their directory', async () => {
      const { selectedProjectsGraph } = setupWorkspace({
        a: { name: 'pkg-a', version: '1.0.0' },
        b: { name: 'pkg-a', version: '2.3.0' },
        c: { name: 'pkg-c', version: '0.1.0' },
      })

      const result = await handler({
        dir: tmpDir,
        workspaceDir: tmpDir,
        recursive: true,
        selectedProjectsGraph,
      }, ['get', 'version'])

      expect(JSON.parse(result as string)).toEqual({
        a: { version: '1.0.0' },
        b: { version: '2.3.0' },
        'pkg-c': { version: '0.1.0' },
      })
    })

    test('runs `set` against every selected workspace package', async () => {
      const { allProjects, selectedProjectsGraph } = setupWorkspace({
        'pkg-a': { name: 'pkg-a', version: '1.0.0' },
        'pkg-b': { name: 'pkg-b', version: '2.0.0' },
      })

      await handler({
        dir: tmpDir,
        workspaceDir: tmpDir,
        recursive: true,
        selectedProjectsGraph,
      }, ['set', 'license=MIT'])

      for (const { rootDir } of allProjects) {
        const updated = JSON.parse(fs.readFileSync(path.join(rootDir, 'package.json'), 'utf8'))
        expect(updated.license).toBe('MIT')
      }
    })

    test('runs `delete` against every selected workspace package', async () => {
      const { allProjects, selectedProjectsGraph } = setupWorkspace({
        'pkg-a': { name: 'pkg-a', version: '1.0.0', extra: 'a' },
        'pkg-b': { name: 'pkg-b', version: '2.0.0', extra: 'b' },
      })

      await handler({
        dir: tmpDir,
        workspaceDir: tmpDir,
        recursive: true,
        selectedProjectsGraph,
      }, ['delete', 'extra'])

      for (const { rootDir } of allProjects) {
        const updated = JSON.parse(fs.readFileSync(path.join(rootDir, 'package.json'), 'utf8'))
        expect(updated.extra).toBeUndefined()
      }
    })

    test('runs against only the selected projects', async () => {
      const { allProjects, selectedProjectsGraph } = setupWorkspace({
        'pkg-a': { name: 'pkg-a', version: '1.0.0' },
        'pkg-b': { name: 'pkg-b', version: '2.0.0' },
        'pkg-c': { name: 'pkg-c', version: '3.0.0' },
      })
      const selected = Object.fromEntries(
        [allProjects[0], allProjects[2]].map((project) => [project.rootDir, selectedProjectsGraph[project.rootDir]])
      )

      await handler({
        dir: tmpDir,
        workspaceDir: tmpDir,
        recursive: true,
        selectedProjectsGraph: selected,
      }, ['set', 'license=MIT'])

      const read = (name: string) =>
        JSON.parse(fs.readFileSync(path.join(tmpDir, name, 'package.json'), 'utf8'))
      expect(read('pkg-a').license).toBe('MIT')
      expect(read('pkg-b').license).toBeUndefined()
      expect(read('pkg-c').license).toBe('MIT')
    })

    test('throws when used outside of a workspace', async () => {
      await expect(handler({ dir: tmpDir, recursive: true }, ['get']))
        .rejects.toMatchObject({ code: 'ERR_PNPM_PKG_RECURSIVE_NO_ROOT' })
    })

    test('throws when no workspace packages were selected', async () => {
      await expect(handler({
        dir: tmpDir,
        workspaceDir: tmpDir,
        recursive: true,
        selectedProjectsGraph: {},
      }, ['get'])).rejects.toMatchObject({ code: 'ERR_PNPM_PKG_RECURSIVE_NO_PACKAGES' })
    })
  })
})
