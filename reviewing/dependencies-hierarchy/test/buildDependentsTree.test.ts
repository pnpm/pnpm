import fs from 'fs'
import os from 'os'
import path from 'path'
import { depPathToFilename, refToRelative } from '@pnpm/dependency-path'
import { type PackageSnapshots, type ProjectSnapshot } from '@pnpm/lockfile.fs'
import { type DepPath, type ProjectId } from '@pnpm/types'
import { buildDependentsTree } from '../lib/buildDependentsTree.js'

function refToRelativeOrThrow (reference: string, pkgName: string): DepPath {
  const relative = refToRelative(reference, pkgName)
  if (relative == null) {
    throw new Error(`Unable to create key for ${pkgName} with reference ${reference}`)
  }
  return relative
}

/**
 * Creates a temporary directory with a minimal virtual store structure so that
 * `buildDependentsTree` can resolve package paths and read manifests.
 *
 * Returns the lockfileDir path and a cleanup function.
 */
function createMockProject (packages: Record<string, { version: string, manifest: Record<string, unknown>, deps?: string[] }>): {
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

    const deps: Record<string, string> = {}
    for (const dep of info.deps ?? []) {
      const depPkg = packages[dep]
      if (depPkg) {
        deps[dep] = refToRelativeOrThrow(depPkg.version, dep)
      }
    }

    currentPackages[depPath] = {
      resolution: { integrity: `${pkgName}-mock-integrity` },
      dependencies: deps,
    }
  }

  // Add leaf packages that are referenced as dependencies but not in the packages map
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

  // Single root importer that depends on all top-level packages
  const rootDeps: Record<string, string> = {}
  for (const [pkgName, info] of Object.entries(packages)) {
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
            const m = manifest as unknown as Record<string, unknown>
            return typeof m.componentName === 'string' ? m.componentName : undefined
          },
        })

        expect(trees).toHaveLength(1)
        // Root tree node should have displayName from the formatter
        expect(trees[0].name).toBe('target')
        expect(trees[0].displayName).toBe('ui/target')

        // The dependents should include mid (which itself depends on target)
        // and the root importer
        const midNode = trees[0].dependents.find(d => d.name === 'mid')
        expect(midNode).toBeDefined()
        expect(midNode!.displayName).toBe('utils/mid')

        // Importer node should not have displayName (nameFormatter is only for packages)
        const importerNode = trees[0].dependents.find(d => d.name === 'my-project')
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
            const m = manifest as unknown as Record<string, unknown>
            return typeof m.componentName === 'string' ? m.componentName : undefined
          },
        })

        expect(trees).toHaveLength(1)
        expect(trees[0].displayName).toBeUndefined()
      } finally {
        cleanup()
      }
    })
  })
})
