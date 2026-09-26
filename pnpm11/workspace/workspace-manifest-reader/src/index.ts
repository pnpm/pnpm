import path from 'node:path'

import { type GLOBAL_CONFIG_YAML_FILENAME, WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import type { PnpmSettings } from '@pnpm/types'
import { readYamlFile, readYamlFileSync } from 'read-yaml-file'

import {
  assertValidWorkspaceManifestCatalog,
  assertValidWorkspaceManifestCatalogs,
  type WorkspaceCatalog,
  type WorkspaceNamedCatalogs,
} from './catalogs.js'
import { InvalidWorkspaceManifestError } from './errors/InvalidWorkspaceManifestError.js'
import { assertValidWorkspaceManifestVersioning } from './versioning.js'

export type ConfigFileName =
  | typeof GLOBAL_CONFIG_YAML_FILENAME
  | typeof WORKSPACE_MANIFEST_FILENAME

export interface WorkspaceManifest extends PnpmSettings {
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

export async function readWorkspaceManifest (dir: string, cfgFileName: ConfigFileName = WORKSPACE_MANIFEST_FILENAME): Promise<WorkspaceManifest | undefined> {
  const manifest = await readManifestRaw(dir, cfgFileName)
  validateWorkspaceManifest(manifest)
  return manifest
}

export function readWorkspaceManifestSync (dir: string, cfgFileName: ConfigFileName = WORKSPACE_MANIFEST_FILENAME): WorkspaceManifest | undefined {
  const manifest = readManifestRawSync(dir, cfgFileName)
  validateWorkspaceManifest(manifest)
  return manifest
}

async function readManifestRaw (dir: string, cfgFileName: ConfigFileName): Promise<unknown> {
  try {
    return await readYamlFile<WorkspaceManifest>(path.join(dir, cfgFileName))
  } catch (err: unknown) {
    // File not exists is the same as empty file (undefined)
    if (isErrorWithCode(err, 'ENOENT')) {
      return undefined
    }

    // Any other error (missing perm, invalid yaml, etc.) fails the process
    throw err
  }
}

function readManifestRawSync (dir: string, cfgFileName: ConfigFileName): unknown {
  try {
    return readYamlFileSync<WorkspaceManifest>(path.join(dir, cfgFileName))
  } catch (err: unknown) {
    if (isErrorWithCode(err, 'ENOENT')) {
      return undefined
    }
    throw err
  }
}

// Some Node.js-compatible runtimes, such as StackBlitz WebContainers, throw fs
// errors that carry a code but are not native errors.
function isErrorWithCode (err: unknown, code: string): boolean {
  return err != null && typeof err === 'object' && 'code' in err && err.code === code
}

export function validateWorkspaceManifest (manifest: unknown): asserts manifest is WorkspaceManifest | undefined {
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
  assertValidWorkspaceManifestVersioning(manifest)

  checkWorkspaceManifestAssignability(manifest)
}

function assertValidWorkspaceManifestPackages (manifest: { packages?: unknown }): asserts manifest is { packages: string[] } {
  if (manifest.packages == null) {
    return
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
