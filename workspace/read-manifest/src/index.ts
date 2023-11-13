import { WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import path from 'node:path'
import readYamlFile from 'read-yaml-file'

export interface WorkspaceManifest {
  packages?: string[]
}

export async function readWorkspaceManifest (dir: string): Promise<WorkspaceManifest | undefined> {
  const manifest = await readManifestRaw(dir)
  if (validateWorkspaceManifest(manifest)) {
    return manifest
  }

  return undefined
}

async function readManifestRaw (dir: string): Promise<unknown> {
  try {
    return await readYamlFile<WorkspaceManifest>(path.join(dir, WORKSPACE_MANIFEST_FILENAME))
  } catch (err: any) { // eslint-disable-line
    // File not exists is the same as empty file (undefined)
    if (err['code'] === 'ENOENT') {
      return undefined
    }

    // Any other error (missing perm, invalid yaml, etc.) fails the process
    throw err
  }
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function validateWorkspaceManifest (manifest: any): manifest is WorkspaceManifest | undefined {
  if (manifest === undefined || manifest === null) {
    // Empty or null manifest is ok
    return true
  }

  if (typeof manifest !== 'object') {
    throw new InvalidWorkspaceManifestError(`Expected object but found - ${typeof manifest}`)
  }

  if (Array.isArray(manifest)) {
    throw new InvalidWorkspaceManifestError('Expected object but found - array')
  }

  if (Object.keys(manifest).length === 0) {
    // manifest content `{}` is ok
    return true
  }

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

  return true
}

class InvalidWorkspaceManifestError extends PnpmError {
  constructor (message: string) {
    super('INVALID_WORKSPACE_CONFIGURATION', message)
  }
}
