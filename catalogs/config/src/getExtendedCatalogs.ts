import path from 'node:path'

import type { Catalogs } from '@pnpm/catalogs.types'
import { PnpmError } from '@pnpm/error'
import { readWorkspaceManifest, type WorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'

import { getCatalogsFromWorkspaceManifest } from './getCatalogsFromWorkspaceManifest.js'
import { mergeCatalogs } from './mergeCatalogs.js'

type ExtendableManifest = Pick<WorkspaceManifest, 'catalog' | 'catalogs' | 'extends'>

/**
 * Resolves the catalogs of a workspace manifest, merging in the catalogs of the
 * workspace manifests it references through the `extends` field.
 *
 * The merge is performed so that the manifest doing the extending wins on
 * conflicts: an entry defined both by the manifest and by one of the manifests
 * it extends keeps the value from the extending manifest. When several manifests
 * are extended, entries from manifests listed later override entries from
 * manifests listed earlier.
 *
 * `extends` is resolved recursively, so an extended manifest may extend other
 * manifests too. Circular references are detected and reported.
 */
export async function getExtendedCatalogs (
  dir: string,
  manifest: ExtendableManifest | undefined
): Promise<Catalogs> {
  return resolveExtendedCatalogs(dir, manifest, [])
}

async function resolveExtendedCatalogs (
  dir: string,
  manifest: ExtendableManifest | undefined,
  ancestors: readonly string[]
): Promise<Catalogs> {
  const ownCatalogs = getCatalogsFromWorkspaceManifest(manifest)
  const extendsPaths = normalizeExtends(manifest?.extends)
  if (extendsPaths.length === 0) {
    return ownCatalogs
  }

  const currentDir = path.resolve(dir)
  if (ancestors.includes(currentDir)) {
    throw new PnpmError(
      'WORKSPACE_EXTENDS_CYCLE',
      `Circular workspace "extends" reference detected. The workspace at "${currentDir}" eventually extends itself.`
    )
  }
  const nextAncestors = [...ancestors, currentDir]

  const extendedCatalogsList = await Promise.all(extendsPaths.map(async (extendsPath) => {
    const extendedDir = path.resolve(dir, extendsPath)
    const extendedManifest = await readWorkspaceManifest(extendedDir)
    if (extendedManifest == null) {
      throw new PnpmError(
        'WORKSPACE_EXTENDS_NOT_FOUND',
        `Cannot find a pnpm-workspace.yaml file in "${extendedDir}", which is referenced by the "extends" field of the workspace at "${currentDir}".`
      )
    }
    return resolveExtendedCatalogs(extendedDir, extendedManifest, nextAncestors)
  }))

  // The extending manifest's own catalogs are merged last so they take
  // precedence over the catalogs coming from the extended manifests.
  return mergeCatalogs(...extendedCatalogsList, ownCatalogs)
}

function normalizeExtends (extendsField: string | string[] | undefined): string[] {
  if (extendsField == null) return []
  return Array.isArray(extendsField) ? extendsField : [extendsField]
}
