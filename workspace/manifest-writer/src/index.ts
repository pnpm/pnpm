import fs from 'fs'
import path from 'path'
import { type Catalogs } from '@pnpm/catalogs.types'
import { type ResolvedCatalogEntry } from '@pnpm/lockfile.types'
import { readWorkspaceManifest, type WorkspaceManifest } from '@pnpm/workspace.read-manifest'
import { WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import writeYamlFile from 'write-yaml-file'
import equals from 'ramda/src/equals'
import { sortKeysByPriority } from '@pnpm/object.key-sorting'
import { findPackages } from '@pnpm/fs.find-packages'

async function writeManifestFile (dir: string, manifest: Partial<WorkspaceManifest>): Promise<void> {
  manifest = sortKeysByPriority({
    priority: { packages: 0 },
    deep: true,
  }, manifest)
  return writeYamlFile(path.join(dir, WORKSPACE_MANIFEST_FILENAME), manifest, {
    lineWidth: -1, // This is setting line width to never wrap
    blankLines: true,
    noCompatMode: true,
    noRefs: true,
    sortKeys: false,
  })
}

export async function updateWorkspaceManifest (dir: string, updatedFields: Partial<WorkspaceManifest> & {
  updatedCatalogs?: Catalogs
  cleanupUnusedCatalogs?: boolean
}): Promise<void> {
  const manifest = await readWorkspaceManifest(dir) ?? {} as WorkspaceManifest
  if (updatedFields.updatedCatalogs ?? updatedFields.cleanupUnusedCatalogs) {
    if (updatedFields.updatedCatalogs) {
      await addCatalogs(dir, manifest, updatedFields.updatedCatalogs)
    }
    if (updatedFields.cleanupUnusedCatalogs) {
      await removePackagesFromWorkspaceCatalog(dir)
    }
    return
  }
  let shouldBeUpdated = false
  for (const [key, value] of Object.entries(updatedFields)) {
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
    await fs.promises.rm(path.join(dir, WORKSPACE_MANIFEST_FILENAME))
    return
  }
  await writeManifestFile(dir, manifest)
}

export interface NewCatalogs {
  [catalogName: string]: {
    [dependencyName: string]: Pick<ResolvedCatalogEntry, 'specifier'>
  }
}

async function addCatalogs (dir: string, manifest: Partial<WorkspaceManifest>, newCatalogs: Catalogs): Promise<void> {
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

  if (shouldBeUpdated) {
    await writeManifestFile(dir, manifest)
  }
}

async function removePackagesFromWorkspaceCatalog (workspaceDir: string): Promise<void> {
  const packagesJson = await findPackages(workspaceDir, {
    includeRoot: true,
  })
  const packages: Record<string, string> = {}
  for (const pkg of packagesJson) {
    const manifest = pkg.manifest
    Object.assign(packages, manifest.dependencies, manifest.devDependencies, manifest.optionalDependencies, manifest.peerDependencies)
  }
  const manifest: Partial<WorkspaceManifest> = await readWorkspaceManifest(workspaceDir) ?? {}
  let shouldBeUpdated = false

  if (manifest.catalog == null && manifest.catalogs == null) return

  if (manifest.catalog != null) {
    Object.keys(manifest.catalog).forEach((pkg) => {
      if (!packages[pkg] || packages[pkg] !== 'catalog:') {
        delete manifest.catalog![pkg]
        shouldBeUpdated = true
      }
    })
    if (Object.keys(manifest.catalog).length === 0) {
      delete manifest.catalog
      shouldBeUpdated = true
    }
  }

  for (const catalogName in manifest.catalogs) {
    const catalog = manifest.catalogs[catalogName]
    if (catalog == null) continue
    Object.keys(catalog).forEach((pkg) => {
      if (!packages[pkg] || (packages[pkg] !== `catalog:${catalogName}` && packages[pkg] !== 'catalog:')) {
        delete catalog[pkg]
        shouldBeUpdated = true
      }
    })
    if (Object.keys(catalog).length === 0) {
      delete manifest.catalogs[catalogName]
      shouldBeUpdated = true
    }
  }
  if (Object.keys(manifest.catalogs ?? {}).length === 0) {
    delete manifest.catalogs
    shouldBeUpdated = true
  }

  if (shouldBeUpdated) {
    await writeManifestFile(workspaceDir, manifest)
  }
}
