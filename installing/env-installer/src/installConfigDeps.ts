import fs from 'node:fs'
import path from 'node:path'

import { checkPackage } from '@pnpm/config.package-is-installable'
import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { installingConfigDepsLogger, skippedOptionalDependencyLogger } from '@pnpm/core-loggers'
import { calcGlobalVirtualStorePathWithSubdeps, calcLeafGlobalVirtualStorePath } from '@pnpm/deps.graph-hasher'
import { PnpmError } from '@pnpm/error'
import { readModulesDir } from '@pnpm/fs.read-modules-dir'
import { type EnvLockfile, readEnvLockfile } from '@pnpm/lockfile.fs'
import { getNpmTarballUrl } from '@pnpm/resolving.tarball-url'
import type { StoreController } from '@pnpm/store.controller'
import type { ConfigDependencies, Registries } from '@pnpm/types'
import { rimraf } from '@zkochan/rimraf'
import { symlinkDir } from 'symlink-dir'

import { migrateConfigDepsToLockfile } from './migrateConfigDeps.js'
import type { NormalizedConfigDep, NormalizedSubdep } from './parseIntegrity.js'
import { verifyEnvLockfile } from './verifyEnvLockfile.js'

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

  let startedEmitted = false
  const reportStarted = (): void => {
    if (startedEmitted) return
    startedEmitted = true
    installingConfigDepsLogger.debug({ status: 'started' })
  }

  await Promise.all(existingConfigDeps.map(async (existingConfigDep) => {
    if (!normalizedDeps[existingConfigDep]) {
      reportStarted()
      await rimraf(path.join(configModulesDir, existingConfigDep))
    }
  }))

  const installedConfigDeps: Array<{ name: string, version: string }> = []
  await Promise.all(Object.entries(normalizedDeps).map(async ([pkgName, pkg]) => {
    const configDepPath = path.join(configModulesDir, pkgName)
    const fullPkgId = `${pkgName}@${pkg.version}:${pkg.resolution.integrity}`
    // The parent's GVS hash must incorporate its optional subdeps; otherwise
    // changing a subdep version while keeping the parent pinned would collide
    // on the same leaf and silently overwrite the previous sibling symlinks.
    const optionalSubdepIds: Record<string, string> = {}
    for (const subdep of pkg.optionalSubdeps ?? []) {
      optionalSubdepIds[subdep.name] = `${subdep.name}@${subdep.version}:${subdep.resolution.integrity}`
    }
    const relPath = calcGlobalVirtualStorePathWithSubdeps(fullPkgId, pkgName, pkg.version, optionalSubdepIds)
    const pkgDirInGlobalVirtualStore = path.join(globalVirtualStoreDir, relPath, 'node_modules', pkgName)
    // The leaf hash captures parent+subdep identities from the lockfile but
    // not the host's `process.arch`/`process.platform` selection. So even if
    // the symlink target is already the expected leaf, the sibling links
    // inside that leaf may target the wrong platform binary if the host's
    // effective arch changed between runs (e.g. Rosetta x64 vs arm64 on
    // macOS). Short-circuit only the parent's re-import/re-symlink in that
    // case; always run installOptionalSubdeps so platform-specific siblings
    // get pruned and relinked.
    const parentSymlinkAlreadyCorrect = existingConfigDeps.includes(pkgName) &&
      await symlinkPointsTo(configDepPath, pkgDirInGlobalVirtualStore)
    if (!fs.existsSync(path.join(pkgDirInGlobalVirtualStore, 'package.json'))) {
      reportStarted()
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
        parentVersion: pkg.version,
        subdeps: pkg.optionalSubdeps,
        // path.dirname would land in the scope subdir for scoped parents; use
        // the leaf's node_modules root so sibling symlinks resolve correctly.
        parentNodeModulesDir: path.join(globalVirtualStoreDir, relPath, 'node_modules'),
        globalVirtualStoreDir,
        rootDir: opts.rootDir,
        store: opts.store,
        reportStarted,
      })
    }
    if (parentSymlinkAlreadyCorrect) {
      return
    }
    reportStarted()
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
    verifyEnvLockfile(configDepsOrLockfile)
    return normalizeFromLockfile(configDepsOrLockfile, opts.registries)
  }

  // It's ConfigDependencies from workspace manifest.
  // Try to read the env lockfile first.
  const envLockfile = await readEnvLockfile(opts.rootDir)
  if (envLockfile) {
    verifyEnvLockfile(envLockfile)
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
  parentVersion: string
  subdeps: NormalizedSubdep[]
  parentNodeModulesDir: string
  globalVirtualStoreDir: string
  rootDir: string
  store: StoreController
  reportStarted: () => void
}

async function installOptionalSubdeps (opts: InstallOptionalSubdepsOpts): Promise<void> {
  const parentLogInfo = { id: `${opts.parentName}@${opts.parentVersion}`, name: opts.parentName, version: opts.parentVersion }
  const compatibleSubdeps = opts.subdeps.filter((subdep) => {
    if (!subdep.os && !subdep.cpu && !subdep.libc) return true
    // Use checkPackage rather than packageIsInstallable: the latter emits a
    // user-visible warn for every incompatible variant, which would fire on
    // every install since the env lockfile records all platform variants for
    // portability. We log skipped subdeps at debug instead.
    const error = checkPackage(
      `${subdep.name}@${subdep.version}`,
      { os: subdep.os, cpu: subdep.cpu, libc: subdep.libc },
      {}
    )
    if (error == null) return true
    skippedOptionalDependencyLogger.debug({
      details: error.toString(),
      package: { id: `${subdep.name}@${subdep.version}`, name: subdep.name, version: subdep.version },
      parents: [parentLogInfo],
      prefix: opts.rootDir,
      reason: error.code === 'ERR_PNPM_UNSUPPORTED_ENGINE' ? 'unsupported_engine' : 'unsupported_platform',
    })
    return false
  })

  const expectedSiblings = new Set([opts.parentName, ...compatibleSubdeps.map((s) => s.name)])
  const existingSiblings = await readModulesDir(opts.parentNodeModulesDir) ?? []
  const orphanSiblings = existingSiblings.filter((name) => !expectedSiblings.has(name))
  if (orphanSiblings.length > 0) {
    opts.reportStarted()
  }
  await Promise.all(orphanSiblings.map((name) => rimraf(path.join(opts.parentNodeModulesDir, name))))

  await Promise.all(compatibleSubdeps.map(async (subdep) => {
    const subdepFullPkgId = `${subdep.name}@${subdep.version}:${subdep.resolution.integrity}`
    const subdepRelPath = calcLeafGlobalVirtualStorePath(subdepFullPkgId, subdep.name, subdep.version)
    const subdepDirInGlobalVirtualStore = path.join(opts.globalVirtualStoreDir, subdepRelPath, 'node_modules', subdep.name)
    if (!fs.existsSync(path.join(subdepDirInGlobalVirtualStore, 'package.json'))) {
      opts.reportStarted()
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
    if (await symlinkPointsTo(linkPath, subdepDirInGlobalVirtualStore)) {
      return
    }
    opts.reportStarted()
    await fs.promises.mkdir(path.dirname(linkPath), { recursive: true })
    await symlinkDir(subdepDirInGlobalVirtualStore, linkPath)
  }))
}

async function symlinkPointsTo (linkPath: string, expectedTarget: string): Promise<boolean> {
  try {
    // Realpath both sides: the expected target itself may live under a
    // symlinked storeDir, and on case-insensitive filesystems the literal
    // string forms can disagree about casing even when they refer to the
    // same inode.
    const [linkReal, targetReal] = await Promise.all([
      fs.promises.realpath(linkPath),
      fs.promises.realpath(expectedTarget),
    ])
    return linkReal === targetReal
  } catch {
    return false
  }
}
