import fs from 'node:fs'
import path from 'node:path'

import { packageIsInstallable } from '@pnpm/config.package-is-installable'
import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { installingConfigDepsLogger } from '@pnpm/core-loggers'
import { calcLeafGlobalVirtualStorePath } from '@pnpm/deps.graph-hasher'
import { PnpmError } from '@pnpm/error'
import { readModulesDir } from '@pnpm/fs.read-modules-dir'
import { type EnvLockfile, readEnvLockfile } from '@pnpm/lockfile.fs'
import { safeReadPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import type { StoreController } from '@pnpm/store.controller'
import type { ConfigDependencies, Registries } from '@pnpm/types'
import { rimraf } from '@zkochan/rimraf'
import getNpmTarballUrl from 'get-npm-tarball-url'
import { symlinkDir } from 'symlink-dir'

import { migrateConfigDepsToLockfile } from './migrateConfigDeps.js'
import type { NormalizedConfigDep, NormalizedSubdep } from './parseIntegrity.js'

export interface InstallConfigDepsOpts {
  frozenLockfile?: boolean
  registries: Registries
  rootDir: string
  store: StoreController
  storeDir: string
}

/**
 * Install config dependencies using the env lockfile.
 * Accepts either a EnvLockfile directly (from resolveConfigDeps) or
 * ConfigDependencies from the workspace manifest (legacy/migration).
 */
export async function installConfigDeps (
  configDepsOrLockfile: ConfigDependencies | EnvLockfile,
  opts: InstallConfigDepsOpts
): Promise<void> {
  const normalizedDeps = await normalizeForInstall(configDepsOrLockfile, opts)
  const globalVirtualStoreDir = path.join(opts.storeDir, 'links')

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
    const existingPkgJson = existingConfigDeps.includes(pkgName)
      ? await safeReadPackageJsonFromDir(configDepPath)
      : null
    if (existingPkgJson != null && existingPkgJson.name === pkgName && existingPkgJson.version === pkg.version) {
      return
    }
    installingConfigDepsLogger.debug({ status: 'started' })
    const fullPkgId = `${pkgName}@${pkg.version}:${pkg.resolution.integrity}`
    // The parent's GVS hash must incorporate its optional subdeps; otherwise
    // changing a subdep version while keeping the parent pinned would collide
    // on the same leaf and silently overwrite the previous sibling symlinks.
    const optionalSubdepIds: Record<string, string> = {}
    for (const subdep of pkg.optionalSubdeps ?? []) {
      optionalSubdepIds[subdep.name] = `${subdep.name}@${subdep.version}:${subdep.resolution.integrity}`
    }
    const relPath = calcLeafGlobalVirtualStorePath(fullPkgId, pkgName, pkg.version, optionalSubdepIds)
    const pkgDirInGlobalVirtualStore = path.join(globalVirtualStoreDir, relPath, 'node_modules', pkgName)
    if (!fs.existsSync(path.join(pkgDirInGlobalVirtualStore, 'package.json'))) {
      const { fetching } = await opts.store.fetchPackage({
        force: true,
        lockfileDir: opts.rootDir,
        pkg: {
          id: `${pkgName}@${pkg.version}`,
          resolution: pkg.resolution,
        },
      })
      const { files: filesResponse } = await fetching()
      await opts.store.importPackage(pkgDirInGlobalVirtualStore, {
        force: true,
        requiresBuild: false,
        filesResponse,
      })
    }
    if (pkg.optionalSubdeps?.length) {
      await installOptionalSubdeps({
        parentName: pkgName,
        subdeps: pkg.optionalSubdeps,
        // path.dirname would land in the scope subdir for scoped parents; use
        // the leaf's node_modules root so sibling symlinks resolve correctly.
        parentNodeModulesDir: path.join(globalVirtualStoreDir, relPath, 'node_modules'),
        globalVirtualStoreDir,
        rootDir: opts.rootDir,
        store: opts.store,
      })
    }
    if (existingConfigDeps.includes(pkgName)) {
      await rimraf(configDepPath)
    }
    await fs.promises.mkdir(path.dirname(configDepPath), { recursive: true })
    await symlinkDir(pkgDirInGlobalVirtualStore, configDepPath)
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
  configDepsOrLockfile: ConfigDependencies | EnvLockfile,
  opts: InstallConfigDepsOpts
): Promise<Record<string, NormalizedConfigDep>> {
  // If it's a EnvLockfile object (has lockfileVersion), use it directly
  if (isEnvLockfile(configDepsOrLockfile)) {
    return normalizeFromLockfile(configDepsOrLockfile, opts.registries)
  }

  // It's ConfigDependencies from workspace manifest.
  // Try to read the env lockfile first.
  const envLockfile = await readEnvLockfile(opts.rootDir)
  if (envLockfile) {
    return normalizeFromLockfile(envLockfile, opts.registries)
  }

  // No env lockfile yet — migrate from old inline integrity format
  if (opts.frozenLockfile) {
    throw new PnpmError('FROZEN_LOCKFILE_WITH_OUTDATED_LOCKFILE', 'Cannot migrate configDependencies with "frozen-lockfile" because the lockfile is not up to date')
  }
  return migrateConfigDepsToLockfile(configDepsOrLockfile, opts)
}

function isEnvLockfile (obj: ConfigDependencies | EnvLockfile): obj is EnvLockfile {
  return 'lockfileVersion' in obj &&
    'importers' in obj &&
    obj.importers != null &&
    typeof obj.importers === 'object' &&
    'packages' in obj &&
    obj.packages != null &&
    typeof obj.packages === 'object' &&
    'snapshots' in obj &&
    obj.snapshots != null &&
    typeof obj.snapshots === 'object'
}

function normalizeFromLockfile (
  lockfile: EnvLockfile,
  registries: Registries
): Record<string, NormalizedConfigDep> {
  const deps: Record<string, NormalizedConfigDep> = {}
  const configDeps = lockfile.importers['.']?.configDependencies ?? {}
  for (const [pkgName, { version }] of Object.entries(configDeps)) {
    const pkgKey = `${pkgName}@${version}`
    const pkgInfo = lockfile.packages[pkgKey]
    if (!pkgInfo) {
      throw new PnpmError(
        'ENV_LOCKFILE_CORRUPTED',
        `pnpm-lock.yaml is corrupted or incomplete: missing packages entry for "${pkgKey}" ` +
        'referenced from importers[\'.\'].configDependencies'
      )
    }
    const resolution = pkgInfo.resolution as { integrity?: string; tarball?: string }
    if (!resolution.integrity) {
      throw new PnpmError(
        'ENV_LOCKFILE_CORRUPTED',
        `pnpm-lock.yaml is corrupted or incomplete: missing integrity for "${pkgKey}"`
      )
    }
    const registry = pickRegistryForPackage(registries, pkgName)
    const snapshot = lockfile.snapshots[pkgKey]
    const optionalSubdeps = snapshot?.optionalDependencies
      ? readOptionalSubdepsFromLockfile(pkgName, snapshot.optionalDependencies, lockfile, registries)
      : undefined
    deps[pkgName] = {
      version,
      resolution: {
        integrity: resolution.integrity,
        tarball: resolution.tarball ?? getNpmTarballUrl(pkgName, version, { registry }),
      },
      optionalSubdeps,
    }
  }
  return deps
}

function readOptionalSubdepsFromLockfile (
  parentName: string,
  optionalDeps: Record<string, string>,
  lockfile: EnvLockfile,
  registries: Registries
): NormalizedSubdep[] {
  const subdeps: NormalizedSubdep[] = []
  for (const [subdepName, subdepVersion] of Object.entries(optionalDeps)) {
    const subdepKey = `${subdepName}@${subdepVersion}`
    const subdepInfo = lockfile.packages[subdepKey]
    if (!subdepInfo) {
      throw new PnpmError(
        'ENV_LOCKFILE_CORRUPTED',
        `pnpm-lock.yaml is corrupted or incomplete: missing packages entry for "${subdepKey}" ` +
        `referenced from optionalDependencies of config dependency "${parentName}"`
      )
    }
    const subdepResolution = subdepInfo.resolution as { integrity?: string; tarball?: string }
    if (!subdepResolution.integrity) {
      throw new PnpmError(
        'ENV_LOCKFILE_CORRUPTED',
        `pnpm-lock.yaml is corrupted or incomplete: missing integrity for "${subdepKey}"`
      )
    }
    const registry = pickRegistryForPackage(registries, subdepName)
    subdeps.push({
      name: subdepName,
      version: subdepVersion,
      resolution: {
        integrity: subdepResolution.integrity,
        tarball: subdepResolution.tarball ?? getNpmTarballUrl(subdepName, subdepVersion, { registry }),
      },
      os: subdepInfo.os,
      cpu: subdepInfo.cpu,
      libc: subdepInfo.libc,
    })
  }
  return subdeps
}

interface InstallOptionalSubdepsOpts {
  parentName: string
  subdeps: NormalizedSubdep[]
  parentNodeModulesDir: string
  globalVirtualStoreDir: string
  rootDir: string
  store: StoreController
}

async function installOptionalSubdeps (opts: InstallOptionalSubdepsOpts): Promise<void> {
  const compatibleSubdeps = opts.subdeps.filter((subdep) => {
    if (!subdep.os && !subdep.cpu && !subdep.libc) return true
    return packageIsInstallable(
      `${subdep.name}@${subdep.version}`,
      { name: subdep.name, version: subdep.version, os: subdep.os, cpu: subdep.cpu, libc: subdep.libc },
      { optional: true, lockfileDir: opts.rootDir }
    ) === true
  })

  const expectedSiblings = new Set([opts.parentName, ...compatibleSubdeps.map((s) => s.name)])
  const existingSiblings = await readModulesDir(opts.parentNodeModulesDir) ?? []
  await Promise.all(existingSiblings
    .filter((name) => !expectedSiblings.has(name))
    .map((name) => rimraf(path.join(opts.parentNodeModulesDir, name))))

  await Promise.all(compatibleSubdeps.map(async (subdep) => {
    const subdepFullPkgId = `${subdep.name}@${subdep.version}:${subdep.resolution.integrity}`
    const subdepRelPath = calcLeafGlobalVirtualStorePath(subdepFullPkgId, subdep.name, subdep.version)
    const subdepDirInGlobalVirtualStore = path.join(opts.globalVirtualStoreDir, subdepRelPath, 'node_modules', subdep.name)
    if (!fs.existsSync(path.join(subdepDirInGlobalVirtualStore, 'package.json'))) {
      const { fetching } = await opts.store.fetchPackage({
        force: true,
        lockfileDir: opts.rootDir,
        pkg: {
          id: `${subdep.name}@${subdep.version}`,
          resolution: subdep.resolution,
        },
      })
      const { files: filesResponse } = await fetching()
      await opts.store.importPackage(subdepDirInGlobalVirtualStore, {
        force: true,
        requiresBuild: false,
        filesResponse,
      })
    }
    const linkPath = path.join(opts.parentNodeModulesDir, subdep.name)
    await fs.promises.mkdir(path.dirname(linkPath), { recursive: true })
    await symlinkDir(subdepDirInGlobalVirtualStore, linkPath)
  }))
}
