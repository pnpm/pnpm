import fs from 'fs'
import path from 'path'
import { type Catalogs } from '@pnpm/catalogs.types'
import { type ResolvedCatalogEntry } from '@pnpm/lockfile.types'
import { readWorkspaceManifest, type WorkspaceManifest } from '@pnpm/workspace.read-manifest'
import { type GLOBAL_CONFIG_YAML_FILENAME, WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import writeYamlFile from 'write-yaml-file'
import { equals } from 'ramda'
import { sortKeysByPriority } from '@pnpm/object.key-sorting'
import {
  type Project,
} from '@pnpm/types'

export type FileName =
  | typeof GLOBAL_CONFIG_YAML_FILENAME
  | typeof WORKSPACE_MANIFEST_FILENAME

const DEFAULT_FILENAME: FileName = WORKSPACE_MANIFEST_FILENAME

async function writeManifestFile (dir: string, fileName: FileName, manifest: Partial<WorkspaceManifest>): Promise<void> {
  manifest = sortKeysByPriority({
    priority: { packages: 0 },
    deep: true,
  }, manifest)
  return writeYamlFile(path.join(dir, fileName), manifest, {
    lineWidth: -1, // This is setting line width to never wrap
    blankLines: true,
    noCompatMode: true,
    noRefs: true,
    sortKeys: false,
  })
}

export async function updateWorkspaceManifest (dir: string, opts: {
  updatedFields?: Partial<WorkspaceManifest>
  updatedCatalogs?: Catalogs
  fileName?: FileName
  cleanupUnusedCatalogs?: boolean
  allProjects?: Project[]
}): Promise<void> {
  const fileName = opts.fileName ?? DEFAULT_FILENAME
  const manifest = await readWorkspaceManifest(dir, fileName) ?? {} as WorkspaceManifest
  let shouldBeUpdated = opts.updatedCatalogs != null && addCatalogs(manifest, opts.updatedCatalogs)
  if (opts.cleanupUnusedCatalogs) {
    shouldBeUpdated = removePackagesFromWorkspaceCatalog(manifest, opts.allProjects ?? []) || shouldBeUpdated
  }

  for (const [key, value] of Object.entries(opts.updatedFields ?? {})) {
    if (!equals(manifest[key as keyof WorkspaceManifest], value)) {
      shouldBeUpdated = true
      if (value == null) {
        delete manifest[key as keyof WorkspaceManifest]
      } else {
        // @ts-expect-error
        manifest[key as keyof WorkspaceManifest] = value
      }
    }
  }
  if (!shouldBeUpdated) {
    return
  }
  if (Object.keys(manifest).length === 0) {
    await fs.promises.rm(path.join(dir, fileName))
    return
  }
  await writeManifestFile(dir, fileName, manifest)
}

export interface NewCatalogs {
  [catalogName: string]: {
    [dependencyName: string]: Pick<ResolvedCatalogEntry, 'specifier'>
  }
}

function addCatalogs (manifest: Partial<WorkspaceManifest>, newCatalogs: Catalogs): boolean {
  let shouldBeUpdated = false

  for (const catalogName in newCatalogs) {
    let targetCatalog: Record<string, string> | undefined = catalogName === 'default'
      ? manifest.catalog ?? manifest.catalogs?.default
      : manifest.catalogs?.[catalogName]
    const targetCatalogWasNil = targetCatalog == null

    for (const [dependencyName, specifier] of Object.entries(newCatalogs[catalogName] ?? {})) {
      if (specifier == null) {
        continue
      }

      targetCatalog ??= {}
      targetCatalog[dependencyName] = specifier
    }

    if (targetCatalog == null) continue

    shouldBeUpdated = true

    if (targetCatalogWasNil) {
      if (catalogName === 'default') {
        manifest.catalog = targetCatalog
      } else {
        manifest.catalogs ??= {}
        manifest.catalogs[catalogName] = targetCatalog
      }
    }
  }

  return shouldBeUpdated
}

function removePackagesFromWorkspaceCatalog (manifest: Partial<WorkspaceManifest>, packagesJson: Project[]): boolean {
  let shouldBeUpdated = false

  if (packagesJson.length === 0 || (manifest.catalog == null && manifest.catalogs == null)) {
    return shouldBeUpdated
  }
  const packageReferences: Record<string, Set<string>> = {}

  for (const pkg of packagesJson) {
    const pkgManifest = pkg.manifest
    const dependencyTypes = [
      pkgManifest.dependencies,
      pkgManifest.devDependencies,
      pkgManifest.optionalDependencies,
      pkgManifest.peerDependencies,
    ]

    for (const deps of dependencyTypes) {
      if (!deps) continue

      for (const [pkgName, version] of Object.entries(deps)) {
        if (!packageReferences[pkgName]) {
          packageReferences[pkgName] = new Set()
        }
        packageReferences[pkgName].add(version)
      }
    }
  }

  if (manifest.catalog) {
    const packagesToRemove = Object.keys(manifest.catalog).filter(pkg =>
      !packageReferences[pkg]?.has('catalog:')
    )

    for (const pkg of packagesToRemove) {
      delete manifest.catalog![pkg]
      shouldBeUpdated = true
    }

    if (Object.keys(manifest.catalog).length === 0) {
      delete manifest.catalog
      shouldBeUpdated = true
    }
  }

  if (manifest.catalogs) {
    const catalogsToRemove: string[] = []

    for (const [catalogName, catalog] of Object.entries(manifest.catalogs)) {
      if (!catalog) continue

      const packagesToRemove = Object.keys(catalog).filter(pkg => {
        const references = packageReferences[pkg]
        return !references?.has(`catalog:${catalogName}`) && !references?.has('catalog:')
      })

      for (const pkg of packagesToRemove) {
        delete catalog[pkg]
        shouldBeUpdated = true
      }

      if (Object.keys(catalog).length === 0) {
        catalogsToRemove.push(catalogName)
        shouldBeUpdated = true
      }
    }

    for (const catalogName of catalogsToRemove) {
      delete manifest.catalogs[catalogName]
    }

    if (Object.keys(manifest.catalogs).length === 0) {
      delete manifest.catalogs
      shouldBeUpdated = true
    }
  }

  return shouldBeUpdated
}
