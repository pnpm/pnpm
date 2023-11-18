import { PnpmError } from '@pnpm/error'
import { filesIncludeInstallScripts } from '@pnpm/exec.files-include-install-scripts'
import { fetch } from '@pnpm/fetch'
import { type ProjectManifest } from '@pnpm/types'

interface RegistryFileFields {
  size: number
  type: 'File'
  path: string
  contentType: string
  hex: string
  isBinary: boolean
  linesCount: number
}

interface RegistryFileList {
  files: {
    [path: string]: RegistryFileFields
  }
}

export async function checkViaRegistryIfPkgRequiresBuild (pkgName: string, pkgVersion: string): Promise<boolean> {
  try {
    const regFS = await fetchPkgIndex(pkgName, pkgVersion)
    const hasInstall = filesIncludeInstallScripts(regFS.files)
    if (hasInstall) return true

    const pkgJsonHex = regFS.files['/package.json'].hex
    const pkgJsonContent = await fetchSpecificFileFromRegistryFS(pkgName, pkgVersion, pkgJsonHex)
    const pkgJson = JSON.parse(pkgJsonContent) as ProjectManifest
    return pkgManifestHasInstallScripts(pkgJson)
  } catch (err) {
    if (err instanceof Error) {
      throw new PnpmError('REG_FS_PARSE', `Failed to fetch ${pkgName}@${pkgVersion}: ${err.message}`)
    }
  }
  return false
}

async function fetchPkgIndex (pkgName: string, pkgVersion: string): Promise<RegistryFileList> {
  const url = `https://npmjs.com/package/${pkgName}/v/${pkgVersion}/index`
  const fetchResult = await fetch(url)
  if (!fetchResult.ok) {
    throw new PnpmError('PKG_INDEX_FETCH', `Failed to fetch ${pkgName}@${pkgVersion} Status: ${fetchResult.statusText}`)
  }

  const fetchJSON = await fetchResult.json() as RegistryFileList
  if (fetchJSON.files == null) {
    throw new PnpmError('PKG_INDEX_FILE_PARSE', `Unable to parse file list for ${pkgName}@${pkgVersion} Status: ${fetchResult.statusText}`)
  }
  return fetchJSON
}

async function fetchSpecificFileFromRegistryFS (pkgName: string, pkgVersion: string, fileHex: string): Promise<string> {
  const url = `https://npmjs.com/package/${pkgName}/file/${fileHex}`
  const fetchResult = await fetch(url)
  if (!fetchResult.ok) {
    throw new PnpmError('REG_FS_ERROR', `Failed to fetch ${pkgName}@${pkgVersion}, file: ${fileHex} Status: ${fetchResult.statusText}`)
  }
  return fetchResult.text()
}

export function pkgManifestHasInstallScripts (manifest: ProjectManifest | undefined): boolean {
  if (!manifest?.scripts) return false
  return Boolean(manifest.scripts.preinstall) ||
    Boolean(manifest.scripts.install) ||
    Boolean(manifest.scripts.postinstall)
}
