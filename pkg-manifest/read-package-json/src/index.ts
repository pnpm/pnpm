import path from 'path'
import { PnpmError } from '@pnpm/error'
import { globalWarn } from '@pnpm/logger'
import { type PackageManifest, type ProjectManifest, type DependenciesField, type DevEngineDependency } from '@pnpm/types'
import { loadJsonFile, loadJsonFileSync } from 'load-json-file'
import normalizePackageData from 'normalize-package-data'

export function readPackageJsonSync (pkgPath: string): PackageManifest {
  try {
    const manifest = loadJsonFileSync<PackageManifest>(pkgPath)
    normalizePackageData(manifest)
    return manifest
  } catch (err: any) { // eslint-disable-line
    if (err.code) throw err
    throw new PnpmError('BAD_PACKAGE_JSON', `${pkgPath}: ${err.message as string}`)
  }
}

export async function readPackageJson (pkgPath: string): Promise<PackageManifest> {
  try {
    const manifest = await loadJsonFile<PackageManifest>(pkgPath)
    normalizePackageData(manifest)
    return manifest
  } catch (err: any) { // eslint-disable-line
    if (err.code) throw err
    throw new PnpmError('BAD_PACKAGE_JSON', `${pkgPath}: ${err.message as string}`)
  }
}

export function readPackageJsonFromDirSync (pkgPath: string): PackageManifest {
  return readPackageJsonSync(path.join(pkgPath, 'package.json'))
}

export async function readPackageJsonFromDir (pkgPath: string): Promise<PackageManifest> {
  return readPackageJson(path.join(pkgPath, 'package.json'))
}

export async function safeReadPackageJson (pkgPath: string): Promise<PackageManifest | null> {
  try {
    return await readPackageJson(pkgPath)
  } catch (err: any) { // eslint-disable-line
    if ((err as NodeJS.ErrnoException).code !== 'ENOENT') throw err
    return null
  }
}

export async function safeReadPackageJsonFromDir (pkgPath: string): Promise<PackageManifest | null> {
  return safeReadPackageJson(path.join(pkgPath, 'package.json'))
}

export function convertEnginesRuntimeToDependencies (manifest: ProjectManifest, enginesFieldName: 'devEngines' | 'engines', dependenciesFieldName: DependenciesField): ProjectManifest {
  for (const runtimeName of ['node', 'deno', 'bun']) {
    if (manifest[enginesFieldName]?.runtime && !manifest[dependenciesFieldName]?.[runtimeName]) {
      const runtimes: DevEngineDependency[] = Array.isArray(manifest[enginesFieldName]!.runtime) ? manifest[enginesFieldName]!.runtime! : [manifest[enginesFieldName]!.runtime!]
      const runtime = runtimes.find((runtime) => runtime.name === runtimeName)
      if (runtime && runtime.onFail === 'download') {
        if ('webcontainer' in process.versions) {
          globalWarn(`Installation of ${runtimeName} versions is not supported in WebContainer`)
        } else {
          manifest[dependenciesFieldName] ??= {}
          manifest[dependenciesFieldName]![runtimeName] = `runtime:${runtime.version}`
        }
      }
    }
  }
  return manifest
}
