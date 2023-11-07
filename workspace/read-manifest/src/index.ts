import { WORKSPACE_MANIFEST_FILENAME } from '@pnpm/constants'
import { PnpmError } from '@pnpm/error'
import path from 'node:path'
import readYamlFile from 'read-yaml-file'

const ERR_CODE = 'INVALID_WORKSPACE_CONFIGURATION'

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
    throw new PnpmError(ERR_CODE, `\n${err.message}`)
  }
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function validateWorkspaceManifest (manifest: any): manifest is WorkspaceManifest | undefined {
  if (manifest === undefined) {
    // Empty manifest is ok
    return true
  }

  if (manifest === null) {
    throw new PnpmError(ERR_CODE, 'Expected object but found - null')
  }

  if (typeof manifest !== 'object') {
    throw new PnpmError(ERR_CODE, `Expected object but found - ${typeof manifest}`)
  }

  if (Array.isArray(manifest)) {
    throw new PnpmError(ERR_CODE, 'Expected object but found - array')
  }

  if (Object.keys(manifest).length === 0) {
    // manifest content `{}` is ok
    return true
  }

  if (!manifest.packages) {
    throw new PnpmError(ERR_CODE, 'packages field missing or empty')
  }

  if (!Array.isArray(manifest.packages)) {
    throw new PnpmError(ERR_CODE, 'packages field is not an array')
  }

  manifest.packages.forEach((pkg: unknown) => {
    if (!pkg) {
      throw new PnpmError(ERR_CODE, 'Missing or empty package')
    }

    const type = typeof pkg
    if (type !== 'string') {
      throw new PnpmError(ERR_CODE, `Invalid package type - ${type}`)
    }
  })

  return true
}
