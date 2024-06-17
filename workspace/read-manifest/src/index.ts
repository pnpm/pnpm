import util from 'util'
import { WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import path from 'node:path'
import readYamlFile from 'read-yaml-file'
import {
  assertValidWorkspaceManifestCatalog,
  assertValidWorkspaceManifestCatalogs,
  type WorkspaceCatalog,
  type WorkspaceNamedCatalogs,
} from './catalogs'
import { InvalidWorkspaceManifestError } from './errors/InvalidWorkspaceManifestError'

export interface WorkspaceManifest {
  packages: string[]

  /**
   * The default catalog. Package manifests may refer to dependencies in this
   * definition through the `catalog:default` specifier or the `catalog:`
   * shorthand.
   */
  catalog?: WorkspaceCatalog

  /**
   * A dictionary of named catalogs. Package manifests may refer to dependencies
   * in this definition through the `catalog:<name>` specifier.
   */
  catalogs?: WorkspaceNamedCatalogs
}

export async function readWorkspaceManifest (dir: string): Promise<WorkspaceManifest | undefined> {
  const manifest = await readManifestRaw(dir)
  validateWorkspaceManifest(manifest)
  return manifest
}

async function readManifestRaw (dir: string): Promise<unknown> {
  try {
    return await readYamlFile<WorkspaceManifest>(path.join(dir, WORKSPACE_MANIFEST_FILENAME))
  } catch (err: unknown) {
    // File not exists is the same as empty file (undefined)
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return undefined
    }

    // Any other error (missing perm, invalid yaml, etc.) fails the process
    throw err
  }
}

function validateWorkspaceManifest (manifest: unknown): asserts manifest is WorkspaceManifest | undefined {
  if (manifest === undefined || manifest === null) {
    // Empty or null manifest is ok
    return
  }

  if (typeof manifest !== 'object') {
    throw new InvalidWorkspaceManifestError(`Expected object but found - ${typeof manifest}`)
  }

  if (Array.isArray(manifest)) {
    throw new InvalidWorkspaceManifestError('Expected object but found - array')
  }

  if (Object.keys(manifest).length === 0) {
    // manifest content `{}` is ok
    return
  }

  assertValidWorkspaceManifestPackages(manifest)
  assertValidWorkspaceManifestCatalog(manifest)
  assertValidWorkspaceManifestCatalogs(manifest)

  checkWorkspaceManifestAssignability(manifest)
}

function assertValidWorkspaceManifestPackages (manifest: { packages?: unknown }): asserts manifest is { packages: string[] } {
  if (!manifest.packages) {
    throw new InvalidWorkspaceManifestError('packages field missing or empty')
  }

  if (!Array.isArray(manifest.packages)) {
    throw new InvalidWorkspaceManifestError('packages field is not an array')
  }

  for (const pkg of manifest.packages) {
    if (!pkg) {
      throw new InvalidWorkspaceManifestError('Missing or empty package')
    }

    const type = typeof pkg
    if (type !== 'string') {
      throw new InvalidWorkspaceManifestError(`Invalid package type - ${type}`)
    }
  }
}

/**
 * Empty function to ensure TypeScript has narrowed the manifest object to
 * something assignable to the {@see WorkspaceManifest} interface. This helps
 * make sure the validation logic in this file is correct as it's refactored in
 * the future.
 */
function checkWorkspaceManifestAssignability (_manifest: WorkspaceManifest): void {}
