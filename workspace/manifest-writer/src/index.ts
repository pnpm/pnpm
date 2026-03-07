import fs from 'fs'
import path from 'path'
import util from 'util'
import { type Catalogs } from '@pnpm/catalogs.types'
import { type ResolvedCatalogEntry } from '@pnpm/lockfile.types'
import { validateWorkspaceManifest, type WorkspaceManifest } from '@pnpm/workspace.read-manifest'
import { WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import { patchDocument } from '@pnpm/yaml.document-sync'
import equals from 'ramda/src/equals'
import yaml from 'yaml'
import writeFileAtomic from 'write-file-atomic'
import { sortKeysByPriority } from '@pnpm/object.key-sorting'
import {
  type Project,
} from '@pnpm/types'

async function writeManifestFile (dir: string, manifest: yaml.Document): Promise<void> {
  const manifestStr = manifest.toString({
    lineWidth: 0, // This is setting line width to never wrap
    singleQuote: true, // Prefer single quotes over double quotes
  })
  await fs.promises.mkdir(dir, { recursive: true })
  await writeFileAtomic(path.join(dir, WORKSPACE_MANIFEST_FILENAME), manifestStr)
}

async function readManifestRaw (file: string): Promise<string | undefined> {
  try {
    return (await fs.promises.readFile(file)).toString()
  } catch (err) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return undefined
    }
    throw err
  }
}

export async function updateWorkspaceManifest (dir: string, opts: {
  updatedFields?: Partial<WorkspaceManifest>
  updatedCatalogs?: Catalogs
  updatedOverrides?: Record<string, string>
  cleanupUnusedCatalogs?: boolean
  allProjects?: Project[]
}): Promise<void> {
  const workspaceManifestStr = await readManifestRaw(path.join(dir, WORKSPACE_MANIFEST_FILENAME))

  const document = workspaceManifestStr != null
    ? yaml.parseDocument(workspaceManifestStr)
    : new yaml.Document()

  let manifest = document.toJSON()
  validateWorkspaceManifest(manifest)
  manifest ??= {}

  let shouldBeUpdated = opts.updatedCatalogs != null && addCatalogs(manifest, opts.updatedCatalogs)
  if (opts.cleanupUnusedCatalogs) {
    shouldBeUpdated = removePackagesFromWorkspaceCatalog(manifest, opts.allProjects ?? []) || shouldBeUpdated
  }

  // If the current manifest has allowBuilds, convert old fields to allowBuilds format
  const updatedFields = { ...opts.updatedFields }
  if (manifest.allowBuilds != null || (manifest.onlyBuiltDependencies == null && manifest.ignoredBuiltDependencies == null)) {
    const allowBuilds: Record<string, boolean | string> = { ...manifest.allowBuilds }

    // Convert onlyBuiltDependencies to allowBuilds with true values
    if (updatedFields.onlyBuiltDependencies != null) {
      for (const pattern of updatedFields.onlyBuiltDependencies) {
        allowBuilds[pattern] = true
      }
    }

    // Convert ignoredBuiltDependencies to allowBuilds with false values
    if (updatedFields.ignoredBuiltDependencies != null) {
      for (const pattern of updatedFields.ignoredBuiltDependencies) {
        allowBuilds[pattern] = false
      }
    }

    // Update allowBuilds instead of the old fields
    if (manifest.allowBuilds != null || Object.keys(allowBuilds).length > 0) {
      updatedFields.allowBuilds = allowBuilds
    }
    delete updatedFields.onlyBuiltDependencies
    delete updatedFields.ignoredBuiltDependencies
  }

  for (const [key, value] of Object.entries(updatedFields)) {
    if (!equals(manifest[key as keyof WorkspaceManifest], value)) {
      shouldBeUpdated = true
      if (value == null) {
        delete manifest[key as keyof WorkspaceManifest]
      } else {
        manifest[key as keyof WorkspaceManifest] = value
      }
    }
  }
  if (opts.updatedOverrides) {
    manifest.overrides ??= {}
    for (const [key, value] of Object.entries(opts.updatedOverrides)) {
      if (!equals(manifest.overrides[key], value)) {
        shouldBeUpdated = true
        manifest.overrides[key] = value
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

  manifest = sortKeysByPriority({
    priority: { packages: 0 },
    deep: true,
  }, manifest)

  patchDocument(document, manifest)

  await writeManifestFile(dir, document)
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
      if (targetCatalog[dependencyName] !== specifier) {
        targetCatalog[dependencyName] = specifier
        shouldBeUpdated = true
      }
    }

    if (targetCatalog == null) continue

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
