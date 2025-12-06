import fs from 'fs'
import path from 'path'
import { type DependenciesGraph, type DependenciesGraphNode } from '@pnpm/resolve-dependencies'
import { PnpmError } from '@pnpm/error'
import { validateOnlyBuiltDependencies } from '../../src/install/validateOnlyBuiltDependencies.js'

function createNode (dir: string, name: string, version: string): DependenciesGraphNode {
  return {
    children: {},
    depth: 0,
    dev: false,
    dir,
    fetching: async () => {
      throw new Error('not used')
    },
    hasBin: false,
    id: `${name}@${version}`,
    independent: false,
    installable: true,
    modules: path.join(dir, 'node_modules'),
    name,
    optional: false,
    optionalDependencies: new Set(),
    patch: undefined,
    requiresBuild: false,
    resolution: { integrity: 'sha512-test' },
    version,
  } as unknown as DependenciesGraphNode
}

function writePkg (dir: string, manifest: object): void {
  fs.mkdirSync(dir, { recursive: true })
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify(manifest), 'utf8')
}

test('does not fail when allow-listed package has lifecycle scripts', async () => {
  const tmpDir = fs.mkdtempSync(path.join(process.cwd(), 'only-built-scripts-'))
  const pkgDir = path.join(tmpDir, 'with-scripts')
  writePkg(pkgDir, {
    name: 'with-scripts',
    version: '1.0.0',
    scripts: {
      install: 'echo hi',
    },
  })

  const graph: DependenciesGraph = {
    '/with-scripts/1.0.0': createNode(pkgDir, 'with-scripts', '1.0.0'),
  }

  await expect(
    validateOnlyBuiltDependencies(graph, {
      onlyBuiltDependencies: ['with-scripts'],
      strictOnlyBuiltDependencies: true,
      lockfileDir: tmpDir,
    })
  ).resolves.toBeUndefined()
})

test('throws when strictOnlyBuiltDependencies is true and allow-listed package has no lifecycle scripts', async () => {
  const tmpDir = fs.mkdtempSync(path.join(process.cwd(), 'only-built-no-scripts-'))
  const pkgDir = path.join(tmpDir, 'no-scripts')
  writePkg(pkgDir, {
    name: 'no-scripts',
    version: '1.0.0',
  })

  const graph: DependenciesGraph = {
    '/no-scripts/1.0.0': createNode(pkgDir, 'no-scripts', '1.0.0'),
  }

  await expect(
    validateOnlyBuiltDependencies(graph, {
      onlyBuiltDependencies: ['no-scripts'],
      strictOnlyBuiltDependencies: true,
      lockfileDir: tmpDir,
    })
  ).rejects.toThrow(PnpmError)
})

test('ignores allow-listed entries that are not resolved in the dependency graph', async () => {
  const tmpDir = fs.mkdtempSync(path.join(process.cwd(), 'only-built-unresolved-'))
  const graph: DependenciesGraph = {}

  await expect(
    validateOnlyBuiltDependencies(graph, {
      onlyBuiltDependencies: ['unresolved-pkg'],
      strictOnlyBuiltDependencies: true,
      lockfileDir: tmpDir,
    })
  ).resolves.toBeUndefined()
})
