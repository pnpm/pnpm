import { PnpmError } from '@pnpm/error'
import { filesIncludeInstallScripts } from '@pnpm/exec.files-include-install-scripts'
import { fetch } from '@pnpm/fetch'
import { type ProjectManifest } from '@pnpm/types'

interface RegistryFileFields {
  path: string
  type: 'file'
  contentType: string
  integrity: string
  lastModified: string
  size: number
}

interface RegistryDirectoryFields {
  path: string
  type: 'directory'
  files: Array<RegistryFileFields | RegistryDirectoryFields>
}

function extractFiles (directory: RegistryDirectoryFields): Record<string, RegistryFileFields> {
  const files: Record<string, RegistryFileFields> = {}

  function traverse (directory: RegistryDirectoryFields) {
    for (const item of directory.files) {
      if (item.type === 'file') {
        files[item.path] = item
      } else {
        traverse(item)
      }
    }
  }

  traverse(directory)
  return files
}
export async function checkViaRegistryIfPkgRequiresBuild (pkgName: string, pkgVersion: string): Promise<boolean> {
  try {
    const unpkgFilesList = await fetchPkgIndex(pkgName, pkgVersion)
    const regFs = extractFiles(unpkgFilesList)
    const hasInstall = filesIncludeInstallScripts(regFs)
    if (hasInstall) return true

    const pkgJsonContent = await fetchSpecificFileFromRegistryFS(pkgName, pkgVersion, 'package.json')
    const pkgJson = JSON.parse(pkgJsonContent) as ProjectManifest
    return pkgManifestHasInstallScripts(pkgJson)
  } catch (err) {
    if (err instanceof Error) {
      throw new PnpmError('REG_FS_PARSE', `Failed to fetch ${pkgName}@${pkgVersion}: ${err.message}`)
    }
  }
  return false
}

async function fetchPkgIndex (pkgName: string, pkgVersion: string): Promise<RegistryDirectoryFields> {
  const url = `https://unpkg.com/${pkgName}@${pkgVersion}/?meta`
  const fetchResult = await fetch(url)
  if (!fetchResult.ok) {
    throw new PnpmError('PKG_INDEX_FETCH', `Failed to fetch ${pkgName}@${pkgVersion} Status: ${fetchResult.statusText}`)
  }

  const fetchJSON = await fetchResult.json() as RegistryDirectoryFields
  if (fetchJSON.files == null) {
    throw new PnpmError('PKG_INDEX_FILE_PARSE', `Unable to parse file list for ${pkgName}@${pkgVersion} Status: ${fetchResult.statusText}`)
  }
  return fetchJSON
}

async function fetchSpecificFileFromRegistryFS (pkgName: string, pkgVersion: string, fileName: string): Promise<string> {
  const url = `https://unpkg.com/${pkgName}@${pkgVersion}/${fileName}`
  const fetchResult = await fetch(url)
  if (!fetchResult.ok) {
    throw new PnpmError('REG_FS_ERROR', `Failed to fetch ${pkgName}@${pkgVersion}, file: ${fileName} Status: ${fetchResult.statusText}`)
  }
  return fetchResult.text()
}

export function pkgManifestHasInstallScripts (manifest: ProjectManifest | undefined): boolean {
  if (!manifest?.scripts) return false
  return Boolean(manifest.scripts.preinstall) ||
    Boolean(manifest.scripts.install) ||
    Boolean(manifest.scripts.postinstall)
}
