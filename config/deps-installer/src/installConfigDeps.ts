import path from 'path'
import getNpmTarballUrl from 'get-npm-tarball-url'
import { installingConfigDepsLogger } from '@pnpm/core-loggers'
import { readModulesDir } from '@pnpm/read-modules-dir'
import rimraf from '@zkochan/rimraf'
import { safeReadPackageJsonFromDir } from '@pnpm/read-package-json'
import type { StoreController } from '@pnpm/package-store'
import type { ConfigDependencies, Registries } from '@pnpm/types'
import { pickRegistryForPackage } from '@pnpm/pick-registry-for-package'
import type { ConfigLockfile } from './configLockfile.js'
import { readConfigLockfile } from './configLockfile.js'
import { migrateConfigDepsToLockfile } from './migrateConfigDeps.js'

export interface InstallConfigDepsOpts {
  registries: Registries
  rootDir: string
  store: StoreController
}

interface NormalizedConfigDep {
  version: string
  resolution: {
    integrity: string
    tarball: string
  }
}

/**
 * Install config dependencies using the config lockfile.
 * Accepts either a ConfigLockfile directly (from resolveConfigDeps) or
 * ConfigDependencies from the workspace manifest (legacy/migration).
 */
export async function installConfigDeps (
  configDepsOrLockfile: ConfigDependencies | ConfigLockfile,
  opts: InstallConfigDepsOpts
): Promise<void> {
  const normalizedDeps = await normalizeForInstall(configDepsOrLockfile, opts)

  const configModulesDir = path.join(opts.rootDir, 'node_modules/.pnpm-config')
  const existingConfigDeps: string[] = await readModulesDir(configModulesDir) ?? []
  await Promise.all(existingConfigDeps.map(async (existingConfigDep) => {
    if (!normalizedDeps[existingConfigDep]) {
      await rimraf(path.join(configModulesDir, existingConfigDep))
    }
  }))

  const installedConfigDeps: Array<{ name: string, version: string }> = []
  await Promise.all(Object.entries(normalizedDeps).map(async ([pkgName, pkg]) => {
    const configDepPath = path.join(configModulesDir, pkgName)
    if (existingConfigDeps.includes(pkgName)) {
      const configDepPkgJson = await safeReadPackageJsonFromDir(configDepPath)
      if (configDepPkgJson == null || configDepPkgJson.name !== pkgName || configDepPkgJson.version !== pkg.version) {
        await rimraf(configDepPath)
      } else {
        return
      }
    }
    installingConfigDepsLogger.debug({ status: 'started' })
    const { fetching } = await opts.store.fetchPackage({
      force: true,
      lockfileDir: opts.rootDir,
      pkg: {
        id: `${pkgName}@${pkg.version}`,
        resolution: pkg.resolution,
      },
    })
    const { files: filesResponse } = await fetching()
    await opts.store.importPackage(configDepPath, {
      force: true,
      requiresBuild: false,
      filesResponse,
    })
    installedConfigDeps.push({
      name: pkgName,
      version: pkg.version,
    })
  }))
  if (installedConfigDeps.length) {
    installingConfigDepsLogger.debug({ status: 'done', deps: installedConfigDeps })
  }
}

async function normalizeForInstall (
  configDepsOrLockfile: ConfigDependencies | ConfigLockfile,
  opts: InstallConfigDepsOpts
): Promise<Record<string, NormalizedConfigDep>> {
  // If it's a ConfigLockfile object (has lockfileVersion), use it directly
  if (isConfigLockfile(configDepsOrLockfile)) {
    return normalizeFromLockfile(configDepsOrLockfile, opts.registries)
  }

  // It's ConfigDependencies from workspace manifest.
  // Try to read the config lockfile first.
  const configLockfile = await readConfigLockfile(opts.rootDir)
  if (configLockfile) {
    return normalizeFromLockfile(configLockfile, opts.registries)
  }

  // No config lockfile yet — migrate from old inline integrity format
  return migrateConfigDepsToLockfile(configDepsOrLockfile, opts)
}

function isConfigLockfile (obj: ConfigDependencies | ConfigLockfile): obj is ConfigLockfile {
  return 'lockfileVersion' in obj
}

function normalizeFromLockfile (
  lockfile: ConfigLockfile,
  registries: Registries
): Record<string, NormalizedConfigDep> {
  const deps: Record<string, NormalizedConfigDep> = {}
  const configDeps = lockfile.importers['.'].configDependencies
  for (const [pkgName, { version }] of Object.entries(configDeps)) {
    const pkgKey = `${pkgName}@${version}`
    const pkgInfo = lockfile.packages[pkgKey]
    if (!pkgInfo) continue
    const registry = pickRegistryForPackage(registries, pkgName)
    deps[pkgName] = {
      version,
      resolution: {
        integrity: pkgInfo.resolution.integrity,
        tarball: pkgInfo.resolution.tarball ?? getNpmTarballUrl(pkgName, version, { registry }),
      },
    }
  }
  return deps
}
