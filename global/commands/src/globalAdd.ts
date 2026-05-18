import fs from 'node:fs'
import path from 'node:path'

import { linkBinsOfPackages } from '@pnpm/bins.linker'
import { removeBin } from '@pnpm/bins.remover'
import type { CommandHandlerMap } from '@pnpm/cli.command'
import { summaryLogger } from '@pnpm/core-loggers'
import {
  cleanOrphanedInstallDirs,
  createGlobalCacheKey,
  createInstallDir,
  findGlobalPackage,
  getHashLink,
  getInstalledBinNames,
} from '@pnpm/global.packages'
import { readPackageJsonFromDirRawSync } from '@pnpm/pkg-manifest.reader'
import type { CreateStoreControllerOptions } from '@pnpm/store.connection-manager'
import { isSubdir } from 'is-subdir'
import { symlinkDir } from 'symlink-dir'

import { checkGlobalBinConflicts } from './checkGlobalBinConflicts.js'
import { installGlobalPackages } from './installGlobalPackages.js'
import { promptApproveGlobalBuilds } from './promptApproveGlobalBuilds.js'
import { readInstalledPackages } from './readInstalledPackages.js'

export type GlobalAddOptions = CreateStoreControllerOptions & {
  bin?: string
  globalPkgDir?: string
  registries: Record<string, string>
  allowBuild?: string[]
  allowBuilds?: Record<string, string | boolean>
  saveExact?: boolean
  savePrefix?: string
  supportedArchitectures?: { libc?: string[] }
  rootProjectManifest?: unknown
}

export async function handleGlobalAdd (
  opts: GlobalAddOptions,
  params: string[],
  commands: CommandHandlerMap
): Promise<void> {
  const globalDir = opts.globalPkgDir!
  const globalBinDir = opts.bin!
  cleanOrphanedInstallDirs(globalDir)

  // Convert allowBuild array to allowBuilds Record (same conversion as add.handler)
  let allowBuilds = opts.allowBuilds ?? {}
  if (opts.allowBuild?.length) {
    allowBuilds = { ...allowBuilds }
    for (const pkg of opts.allowBuild) {
      allowBuilds[pkg] = true
    }
  }

  // Each space-separated CLI param becomes its own isolated install group.
  // A param containing commas is split into multiple selectors that share a
  // single group, so `pnpm add -g foo,bar qar` installs foo+bar together
  // and qar separately. Local paths and URLs that legitimately contain
  // commas are detected and kept whole.
  const groups = params
    .map((param) => splitCommaSeparated(param, opts.dir).map((token) => resolveLocalParam(token, opts.dir)))
    .filter((group) => group.length > 0)

  for (const group of groups) {
    // eslint-disable-next-line no-await-in-loop
    await installGroup({ opts, globalDir, globalBinDir, allowBuilds, params: group }, commands)
  }

  // The per-group `mutateModulesInSingleProject` calls run with
  // `omitSummaryLog: true` so the default-reporter's summary block only
  // appears once at the end, with every installed package listed under a
  // single "global:" heading. Without this, the reporter would print
  // group 1's summary and then ignore later groups, because its summary
  // pipeline takes only the first `summary` log event.
  summaryLogger.debug({ prefix: globalDir })
}

interface InstallGroupContext {
  opts: GlobalAddOptions
  globalDir: string
  globalBinDir: string
  allowBuilds: Record<string, string | boolean>
  params: string[]
}

async function installGroup (
  ctx: InstallGroupContext,
  commands: CommandHandlerMap
): Promise<void> {
  const { opts, globalDir, globalBinDir, allowBuilds, params } = ctx

  // Install into a new directory first, then read the resolved aliases
  // from the resulting package.json. This is more reliable than parsing
  // aliases from CLI params (which may be tarballs, git URLs, etc.).
  const installDir = createInstallDir(globalDir)

  const include = {
    dependencies: true,
    devDependencies: false,
    optionalDependencies: true,
  }
  const fetchFullMetadata = opts.supportedArchitectures?.libc != null && true

  const installOpts = {
    ...opts,
    global: false,
    bin: path.join(installDir, 'node_modules/.bin'),
    dir: installDir,
    lockfileDir: installDir,
    rootProjectManifestDir: installDir,
    rootProjectManifest: undefined,
    saveProd: true,
    saveDev: false,
    saveOptional: false,
    savePeer: false,
    workspaceDir: undefined,
    sharedWorkspaceLockfile: false,
    lockfileOnly: false,
    fetchFullMetadata,
    include,
    includeDirect: include,
    allowBuilds,
    omitSummaryLog: true,
  }

  const ignoredBuilds = await installGlobalPackages(installOpts, params)

  await promptApproveGlobalBuilds({
    globalPkgDir: globalDir,
    installDir,
    ignoredBuilds,
    allowBuilds,
    inheritedOpts: opts,
  }, commands)

  // Read resolved aliases from the installed package.json
  const pkgJson = readPackageJsonFromDirRawSync(installDir)
  const aliases = Object.keys(pkgJson.dependencies ?? {})

  // Check for bin name conflicts with other global packages
  // (must happen before removeExistingGlobalInstalls so we don't lose existing packages on failure)
  const pkgs = await readInstalledPackages(installDir)
  let binsToSkip: Set<string>
  try {
    binsToSkip = await checkGlobalBinConflicts({
      globalDir,
      globalBinDir,
      newPkgs: pkgs,
      shouldSkip: (pkg) => aliases.some((alias) => alias in pkg.dependencies),
    })
  } catch (err) {
    await fs.promises.rm(installDir, { recursive: true, force: true })
    throw err
  }

  // Remove any existing global installations of these aliases
  await removeExistingGlobalInstalls(globalDir, globalBinDir, aliases)

  // Compute cache key and create hash symlink pointing to install dir
  const cacheHash = createGlobalCacheKey({
    aliases,
    registries: opts.registries,
  })
  const hashLink = getHashLink(globalDir, cacheHash)
  await symlinkDir(installDir, hashLink, { overwrite: true })

  // Link bins from installed packages into global bin dir
  await linkBinsOfPackages(pkgs, globalBinDir, { excludeBins: binsToSkip })
}

function splitCommaSeparated (param: string, baseDir: string): string[] {
  if (!param.includes(',')) return [param]
  // URLs may contain commas and are never a group of selectors.
  if (param.includes('://')) return [param]
  // For path-like specs (relative/absolute paths, file:, link:), the
  // commas could either be part of a single path that legitimately
  // contains commas, or be separators between multiple distinct paths.
  // Resolve the ambiguity by checking whether the whole param actually
  // refers to an existing local path on disk.
  if (refersToExistingLocalPath(param, baseDir)) return [param]
  return param.split(',').map((token) => token.trim()).filter(Boolean)
}

function refersToExistingLocalPath (param: string, baseDir: string): boolean {
  let pathPart: string
  if (param.startsWith('file:')) {
    pathPart = param.slice('file:'.length)
  } else if (param.startsWith('link:')) {
    pathPart = param.slice('link:'.length)
  } else if (param[0] === '.' || param[0] === '/' || param[0] === '~') {
    pathPart = param
  } else if (/^[a-z]:[/\\]/i.test(param)) {
    pathPart = param
  } else {
    return false
  }
  const resolved = path.isAbsolute(pathPart) ? pathPart : path.resolve(baseDir, pathPart)
  try {
    fs.statSync(resolved)
    return true
  } catch {
    return false
  }
}

function resolveLocalParam (param: string, baseDir: string): string {
  for (const prefix of ['file:', 'link:']) {
    if (param.startsWith(prefix)) {
      const rest = param.slice(prefix.length)
      if (rest.startsWith('.')) {
        return prefix + path.resolve(baseDir, rest)
      }
      return param
    }
  }
  if (param.startsWith('.')) {
    return path.resolve(baseDir, param)
  }
  return param
}

async function removeExistingGlobalInstalls (
  globalDir: string,
  globalBinDir: string,
  aliases: string[]
): Promise<void> {
  // Collect unique groups to remove (dedup by hash)
  const groupsToRemove = new Map<string, ReturnType<typeof getInstalledBinNames>>()
  for (const alias of aliases) {
    const existing = findGlobalPackage(globalDir, alias)
    if (existing && !groupsToRemove.has(existing.hash)) {
      groupsToRemove.set(existing.hash, getInstalledBinNames(existing))
    }
  }

  // Remove all groups in parallel
  await Promise.all(
    [...groupsToRemove.entries()].map(async ([hash, binNamesPromise]) => {
      const binNames = await binNamesPromise
      await Promise.all(binNames.map((binName) => removeBin(path.join(globalBinDir, binName))))
      // Remove both the hash symlink and the install dir it points to
      const hashLink = getHashLink(globalDir, hash)
      let installDir: string | null = null
      try {
        installDir = fs.realpathSync(hashLink)
      } catch {}
      await fs.promises.rm(hashLink, { force: true })
      if (installDir && isSubdir(globalDir, installDir)) {
        await fs.promises.rm(installDir, { recursive: true, force: true })
      }
    })
  )
}
