import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'

import { describe, expect, test } from '@jest/globals'
import { depPathToFilename, refToRelative } from '@pnpm/deps.path'
import type { PackageSnapshots, ProjectSnapshot } from '@pnpm/lockfile.fs'
import type { DepPath, ProjectId } from '@pnpm/types'

import { buildDependentsTree } from '../lib/buildDependentsTree.js'
import { getPackageMergeId } from '../lib/dependentsTreeIdentity.js'

function refToRelativeOrThrow (reference: string, pkgName: string): DepPath {
  const relative = refToRelative(reference, pkgName)
  if (relative == null) {
    throw new Error(`Unable to create key for ${pkgName} with reference ${reference}`)
  }
  return relative
}

type MockPackages = Record<string, {
  version: string
  manifest: Record<string, unknown>
  deps?: string[]
  peerDeps?: string[]
  topLevel?: boolean
}>

function resolveMockDependencies (depNames: string[], packages: MockPackages): Record<string, string> {
  const deps: Record<string, string> = {}
  for (const dep of depNames) {
    const depPkg = packages[dep]
    if (depPkg) {
      deps[dep] = refToRelativeOrThrow(depPkg.version, dep)
    }
  }
  return deps
}

/**
 * Adds leaf packages that are referenced as dependencies but not in the packages map.
 */
function addMissingLeafPackages (packages: MockPackages, currentPackages: PackageSnapshots): void {
  for (const [, info] of Object.entries(packages)) {
    for (const dep of info.deps ?? []) {
      const depPath = refToRelativeOrThrow(packages[dep]?.version ?? '1.0.0', dep)
      if (currentPackages[depPath] == null) {
        currentPackages[depPath] = {
          resolution: { integrity: `${dep}-mock-integrity` },
          dependencies: {},
        }
      }
    }
  }
}

/**
 * Creates a temporary directory with a minimal virtual store structure so that
 * `buildDependentsTree` can resolve package paths and read manifests.
 *
 * Returns the lockfileDir path and a cleanup function.
 */
function createMockProject (packages: MockPackages): {
  lockfileDir: string
  currentPackages: PackageSnapshots
  importers: Record<ProjectId, ProjectSnapshot>
  cleanup: () => void
} {
  const lockfileDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-test-'))
  const virtualStoreDir = path.join(lockfileDir, 'node_modules', '.pnpm')
  const currentPackages: PackageSnapshots = {}

  for (const [pkgName, info] of Object.entries(packages)) {
    const depPath = refToRelativeOrThrow(info.version, pkgName)
    const depFilename = depPathToFilename(depPath, 120)
    const pkgDir = path.join(virtualStoreDir, depFilename, 'node_modules', pkgName)
    fs.mkdirSync(pkgDir, { recursive: true })
    fs.writeFileSync(path.join(pkgDir, 'package.json'), JSON.stringify({
      name: pkgName,
      version: info.version,
      ...info.manifest,
    }))

    currentPackages[depPath] = {
      resolution: { integrity: `${pkgName}-mock-integrity` },
      dependencies: resolveMockDependencies(info.deps ?? [], packages),
      ...(info.peerDeps && { peerDependencies: Object.fromEntries(info.peerDeps.map((dep) => [dep, '*'])) }),
    }
  }

  addMissingLeafPackages(packages, currentPackages)

  // Single root importer that depends on all top-level packages
  const rootDeps: Record<string, string> = {}
  for (const [pkgName, info] of Object.entries(packages)) {
    if (info.topLevel === false) continue
    rootDeps[pkgName] = info.version
  }

  const importers = {
    '.': {
      dependencies: rootDeps,
      specifiers: {},
    },
  } as Record<ProjectId, ProjectSnapshot>

  return {
    lockfileDir,
    currentPackages,
    importers,
    cleanup: () => {
      fs.rmSync(lockfileDir, { recursive: true, force: true })
    },
  }
}

describe('buildDependentsTree', () => {
  describe('excludePeerDependencies', () => {
    async function whyTree (pkg: string, project: ReturnType<typeof createMockProject>, excludePeerDependencies: boolean) {
      return buildDependentsTree([pkg], [project.lockfileDir], {
        lockfileDir: project.lockfileDir,
        excludePeerDependencies,
        importerInfoMap: new Map([['.', { name: 'my-project', version: '0.0.0' }]]),
        lockfile: {
          lockfileVersion: '9.0',
          importers: project.importers,
          packages: project.currentPackages,
        },
      })
    }

    test('drops packages that are reached only as a resolved peer, along with their own dependencies', async () => {
      const project = createMockProject({
        parent: { version: '1.0.0(peer@1.0.0)', manifest: {}, deps: ['peer'], peerDeps: ['peer'] },
        peer: { version: '1.0.0', manifest: {}, deps: ['peer-dep'], topLevel: false },
        'peer-dep': { version: '1.0.0', manifest: {}, topLevel: false },
      })
      try {
        expect(await whyTree('peer', project, true)).toStrictEqual([])
        expect(await whyTree('peer-dep', project, true)).toStrictEqual([])
        expect((await whyTree('peer', project, false))[0].dependents.map(({ name }) => name)).toStrictEqual(['parent'])
        expect((await whyTree('peer-dep', project, false))[0].dependents.map(({ name }) => name)).toStrictEqual(['peer'])
      } finally {
        project.cleanup()
      }
    })

    test('keeps a dependency that is also declared as a peer when the key does not bind it', async () => {
      const project = createMockProject({
        wrapper: { version: '1.0.0', manifest: {}, deps: ['mid'], peerDeps: ['mid'] },
        mid: { version: '1.0.0', manifest: {}, topLevel: false },
      })
      try {
        expect((await whyTree('mid', project, true))[0].dependents.map(({ name }) => name)).toStrictEqual(['wrapper'])
      } finally {
        project.cleanup()
      }
    })

    test('keeps the non-peer paths to a package that is also a resolved peer', async () => {
      const project = createMockProject({
        parent: { version: '1.0.0(peer@1.0.0)', manifest: {}, deps: ['peer'], peerDeps: ['peer'] },
        peer: { version: '1.0.0', manifest: {} },
      })
      try {
        const trees = await whyTree('peer', project, true)

        expect(trees).toHaveLength(1)
        expect(trees[0].dependents).toStrictEqual([
          { name: 'my-project', version: '0.0.0', depField: 'dependencies' },
        ])
      } finally {
        project.cleanup()
      }
    })
  })

  test('a devDependency that only satisfies an optional peer has no dependents in a production tree', async () => {
    // The key records both peers as bindings, as a resolved lockfile does.
    const ABC_REF = '1.0.0(peer-a@1.0.0)(peer-c@1.0.0)'
    const { lockfileDir, currentPackages, importers, cleanup } = createMockProject({
      abc: { version: ABC_REF, manifest: {}, deps: ['peer-a', 'peer-c'] },
      'peer-a': { version: '1.0.0', manifest: {} },
      'peer-c': { version: '1.0.0', manifest: {} },
    })
    try {
      Object.assign(currentPackages[`abc@${ABC_REF}` as DepPath], {
        peerDependencies: { 'peer-a': '^1.0.0', 'peer-c': '^1.0.0' },
        peerDependenciesMeta: { 'peer-c': { optional: true } },
      })
      importers['.' as ProjectId] = {
        dependencies: { abc: ABC_REF },
        devDependencies: { 'peer-a': '1.0.0', 'peer-c': '1.0.0' },
        specifiers: {},
      }
      const whyTree = async (pkg: string, include?: { dependencies: boolean, devDependencies: boolean, optionalDependencies: boolean }) => buildDependentsTree([pkg], [lockfileDir], {
        lockfileDir,
        include,
        importerInfoMap: new Map([['.', { name: 'my-project', version: '0.0.0' }]]),
        lockfile: {
          lockfileVersion: '9.0',
          importers,
          packages: currentPackages,
        },
      })
      const prodOnly = { dependencies: true, devDependencies: false, optionalDependencies: true }

      expect(await whyTree('peer-c', prodOnly)).toHaveLength(0)
      // A required peer stays even when only a devDependency provides it.
      expect((await whyTree('peer-a', prodOnly))[0].dependents.map(({ name }) => name)).toStrictEqual(['abc'])
      expect((await whyTree('peer-c'))[0].dependents.map(({ name }) => name)).toContain('abc')
    } finally {
      cleanup()
    }
  })

  test('a dependency named like an Object.prototype property is reported under its own field', async () => {
    const { lockfileDir, currentPackages, importers, cleanup } = createMockProject({
      constructor: { version: '1.0.0', manifest: {} },
    })
    try {
      importers['.' as ProjectId] = {
        dependencies: { constructor: '1.0.0' },
        devDependencies: {},
        optionalDependencies: {},
        specifiers: {},
      }
      const trees = await buildDependentsTree(['constructor'], [lockfileDir], {
        lockfileDir,
        importerInfoMap: new Map([['.', { name: 'my-project', version: '0.0.0' }]]),
        lockfile: {
          lockfileVersion: '9.0',
          importers,
          packages: currentPackages,
        },
      })

      expect(trees[0].dependents).toStrictEqual([
        { name: 'my-project', version: '0.0.0', depField: 'dependencies' },
      ])
    } finally {
      cleanup()
    }
  })

  describe('nameFormatter', () => {
    test('populates displayName on matched root and intermediate nodes', async () => {
      const { lockfileDir, currentPackages, importers, cleanup } = createMockProject({
        target: {
          version: '1.0.0',
          manifest: { componentName: 'ui/target' },
        },
        mid: {
          version: '2.0.0',
          manifest: { componentName: 'utils/mid' },
          deps: ['target'],
        },
      })

      try {
        // mid depends on target; root importer depends on both
        const importerInfoMap = new Map([
          ['.', { name: 'my-project', version: '0.0.0' }],
        ])

        const trees = await buildDependentsTree(['target'], [lockfileDir], {
          lockfileDir,
          importerInfoMap,
          lockfile: {
            lockfileVersion: '9.0',
            importers,
            packages: currentPackages,
          },
          nameFormatter: ({ manifest }) => {
            const manifestFields = manifest as unknown as Record<string, unknown>
            return typeof manifestFields.componentName === 'string' ? manifestFields.componentName : undefined
          },
        })

        expect(trees).toHaveLength(1)
        // Root tree node should have displayName from the formatter
        expect(trees[0].name).toBe('target')
        expect(trees[0].displayName).toBe('ui/target')

        // The dependents should include mid (which itself depends on target)
        // and the root importer
        const midNode = trees[0].dependents.find(dependent => dependent.name === 'mid')
        expect(midNode).toBeDefined()
        expect(midNode!.displayName).toBe('utils/mid')

        // Importer node should not have displayName (nameFormatter is only for packages)
        const importerNode = trees[0].dependents.find(dependent => dependent.name === 'my-project')
        if (importerNode) {
          expect(importerNode.displayName).toBeUndefined()
        }
      } finally {
        cleanup()
      }
    })

    test('displayName is undefined when nameFormatter is not provided', async () => {
      const { lockfileDir, currentPackages, importers, cleanup } = createMockProject({
        target: {
          version: '1.0.0',
          manifest: { componentName: 'ui/target' },
        },
      })

      try {
        const importerInfoMap = new Map([
          ['.', { name: 'my-project', version: '0.0.0' }],
        ])

        const trees = await buildDependentsTree(['target'], [lockfileDir], {
          lockfileDir,
          importerInfoMap,
          lockfile: {
            lockfileVersion: '9.0',
            importers,
            packages: currentPackages,
          },
        })

        expect(trees).toHaveLength(1)
        expect(trees[0].displayName).toBeUndefined()
      } finally {
        cleanup()
      }
    })

    test('displayName is undefined when nameFormatter returns undefined', async () => {
      const { lockfileDir, currentPackages, importers, cleanup } = createMockProject({
        target: {
          version: '1.0.0',
          manifest: {},
        },
      })

      try {
        const importerInfoMap = new Map([
          ['.', { name: 'my-project', version: '0.0.0' }],
        ])

        const trees = await buildDependentsTree(['target'], [lockfileDir], {
          lockfileDir,
          importerInfoMap,
          lockfile: {
            lockfileVersion: '9.0',
            importers,
            packages: currentPackages,
          },
          nameFormatter: ({ manifest }) => {
            const manifestFields = manifest as unknown as Record<string, unknown>
            return typeof manifestFields.componentName === 'string' ? manifestFields.componentName : undefined
          },
        })

        expect(trees).toHaveLength(1)
        expect(trees[0].displayName).toBeUndefined()
      } finally {
        cleanup()
      }
    })
  })

  test('getPackageMergeId resolves directory paths lexically and normalizes UNC and trailing slashes without I/O', () => {
    const project1MergeId = getPackageMergeId({
      lockfileDir: path.join('/workspace', 'packages', 'project-1'),
      depPath: 'foo@file:../../vendor/foo-v1' as DepPath,
      snapshot: { resolution: { type: 'directory', directory: '../../vendor/foo-v1' } },
      version: undefined,
    })
    const project1TrailingSlashMergeId = getPackageMergeId({
      lockfileDir: path.join('/workspace', 'packages', 'project-1'),
      depPath: 'foo@file:../../vendor/foo-v1/' as DepPath,
      snapshot: { resolution: { type: 'directory', directory: '../../vendor/foo-v1/' } },
      version: undefined,
    })
    const project2MergeId = getPackageMergeId({
      lockfileDir: path.join('/workspace', 'packages', 'nested', 'project-2'),
      depPath: 'foo@file:../../../vendor/foo-v1' as DepPath,
      snapshot: { resolution: { type: 'directory', directory: '../../../vendor/foo-v1' } },
      version: undefined,
    })
    const distinctMergeId = getPackageMergeId({
      lockfileDir: path.join('/workspace', 'packages', 'project-3'),
      depPath: 'foo@file:../../vendor/foo-v2' as DepPath,
      snapshot: { resolution: { type: 'directory', directory: '../../vendor/foo-v2' } },
      version: undefined,
    })
    expect(project1MergeId).toBe(project1TrailingSlashMergeId)
    expect(project1MergeId).toBe(project2MergeId)
    expect(project1MergeId).not.toBe(distinctMergeId)

    if (path.sep === '/') {
      const literalBackslashMergeId = getPackageMergeId({
        lockfileDir: path.join('/workspace', 'packages', 'project-1'),
        depPath: 'foo@file:../../vendor/foo-v1\\' as DepPath,
        snapshot: { resolution: { type: 'directory', directory: '../../vendor/foo-v1\\' } },
        version: undefined,
      })
      expect(literalBackslashMergeId).not.toBe(project1MergeId)
    }

    expect(getPackageMergeId({
      lockfileDir: path.join('/workspace', 'packages', 'project-1'),
      depPath: 'root@file:/' as DepPath,
      snapshot: { resolution: { type: 'directory', directory: '/' } },
      version: undefined,
    })).toBe(`dir:${path.normalize('/')}`)

    const uncBackslash = getPackageMergeId({
      lockfileDir: path.join('/workspace', 'packages', 'project-1'),
      depPath: 'foo@file://server/share/foo' as DepPath,
      snapshot: { resolution: { type: 'directory', directory: '\\\\server\\share\\foo' } },
      version: undefined,
    })
    const uncForwardSlash = getPackageMergeId({
      lockfileDir: path.join('/workspace', 'packages', 'project-1'),
      depPath: 'foo@file://server/share/foo' as DepPath,
      snapshot: { resolution: { type: 'directory', directory: '//server/share/foo/' } },
      version: undefined,
    })
    expect(uncBackslash).toBe('dir://server/share/foo')
    expect(uncBackslash).toBe(uncForwardSlash)
  })
})
