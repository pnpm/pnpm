import fs from 'node:fs'
import path from 'node:path'

import type { CommandHandlerMap } from '@pnpm/cli.command'
import { summaryLogger } from '@pnpm/core-loggers'
import { PnpmError } from '@pnpm/error'
import {
  cleanOrphanedInstallDirs,
  createGlobalCacheKey,
  createInstallDir,
  findGlobalPackage,
  getHashLink,
  type GlobalPackageBinSnapshot,
  type GlobalPackageInfo,
} from '@pnpm/global.packages'
import { readPackageJsonFromDirRawSync } from '@pnpm/pkg-manifest.reader'
import type { CreateStoreControllerOptions } from '@pnpm/store.connection-manager'

import { getGlobalBinOwnership } from './binOwnership.js'
import { checkGlobalBinConflicts } from './checkGlobalBinConflicts.js'
import { cleanupFailedGlobalInstall } from './cleanupFailedGlobalInstall.js'
import { activateGlobalInstall, cleanupReplacedGlobalInstalls, getActualBinNames } from './globalActivation.js'
import { installGlobalPackages, type ResolutionPolicyViolation } from './installGlobalPackages.js'
import { isPnpmCliDependency, isPnpmCliOnlyGroup, selectsPnpmCli } from './pnpmCliPackages.js'
import { promptApproveGlobalBuilds } from './promptApproveGlobalBuilds.js'
import { type InstalledGroupPackage, readInstalledPackages } from './readInstalledPackages.js'

export type GlobalAddOptions = CreateStoreControllerOptions & {
  bin?: string
  globalPkgDir?: string
  registriesByScope: Record<string, string>
  /** Already merged with the `--allow-build` selectors by `add`'s handler. */
  allowBuilds?: Record<string, string | boolean>
  saveExact?: boolean
  savePrefix?: string
  rootProjectManifest?: unknown
  handleResolutionPolicyViolations?: (violations: readonly ResolutionPolicyViolation[]) => Promise<void>
  updateResolutionPolicyManifest?: (violations: readonly ResolutionPolicyViolation[], dir: string) => Promise<void>
}

export async function handleGlobalAdd (
  opts: GlobalAddOptions,
  params: string[],
  commands: CommandHandlerMap
): Promise<void> {
  const globalDir = opts.globalPkgDir!
  const globalBinDir = opts.bin!
  cleanOrphanedInstallDirs(globalDir)

  const allowBuilds = opts.allowBuilds ?? {}

  // Each space-separated CLI param becomes its own isolated install group.
  // A param containing commas is split into multiple selectors that share a
  // single group, so `pnpm add -g foo,bar qar` installs foo+bar together
  // and qar separately. Local paths and URLs that legitimately contain
  // commas are detected and kept whole.
  const groups = params
    .map((param) => splitCommaSeparated(param, opts.dir).map((token) => resolveLocalParam(token, opts.dir)))
    .filter((group) => group.length > 0)
  // The rule applies to what actually gets installed, so it runs on the tokens
  // a comma-separated group splits into rather than on the group: `pnpm,lodash`
  // is a request to install pnpm. See `isPnpmCliDependency`.
  if (selectsPnpmCli(groups.flat())) {
    throw new PnpmError('GLOBAL_PNPM_INSTALL', 'Use the "pnpm self-update" command to install or update pnpm')
  }

  for (const group of groups) {
    // eslint-disable-next-line no-await-in-loop -- groups share the global bin directory, so they are installed one at a time
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

export interface InstallGroupContext {
  opts: GlobalAddOptions
  globalDir: string
  globalBinDir: string
  allowBuilds: Record<string, string | boolean>
  params: string[]
}

export async function installGroup (
  ctx: InstallGroupContext,
  commands: CommandHandlerMap
): Promise<void> {
  const { opts, globalDir, allowBuilds, params } = ctx

  // Install into a new directory first, then read the resolved aliases
  // from the resulting package.json. This is more reliable than parsing
  // aliases from CLI params (which may be tarballs, git URLs, etc.).
  const installDir = createInstallDir(globalDir)

  const { ignoredBuilds, resolutionPolicyViolations } = await installGlobalPackages(createGroupInstallOptions(ctx, installDir), params)

  await promptApproveGlobalBuilds({
    globalPkgDir: globalDir,
    installDir,
    ignoredBuilds,
    allowBuilds,
    inheritedOpts: opts,
  }, commands)

  const pkgJson = readPackageJsonFromDirRawSync(installDir)
  const aliases = Object.keys(pkgJson.dependencies ?? {})

  const pkgs = await readInstalledPackages(installDir)
  const plan = await planGroupActivation({ ctx, installDir, aliases, pkgs })
  await activateGroup({ ctx, installDir, aliases, pkgs, plan })
  await opts.updateResolutionPolicyManifest?.(resolutionPolicyViolations, globalDir)
}

function createGroupInstallOptions (ctx: InstallGroupContext, installDir: string) {
  const include = {
    dependencies: true,
    devDependencies: false,
    optionalDependencies: true,
  }

  return {
    ...ctx.opts,
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
    include,
    includeDirect: include,
    allowBuilds: ctx.allowBuilds,
    omitSummaryLog: true,
  }
}

interface GroupActivationInput {
  ctx: InstallGroupContext
  installDir: string
  aliases: string[]
  pkgs: InstalledGroupPackage[]
}

interface GroupActivationPlan {
  binsToSkip: Set<string>
  retainedBinNames: Set<string>
  existingGlobalInstalls: ExistingGlobalInstalls
}

async function planGroupActivation (input: GroupActivationInput): Promise<GroupActivationPlan> {
  const { ctx, installDir, aliases, pkgs } = input
  const replacementAliases = getReplacementAliases(aliases)
  try {
    const binsToSkip = await checkGlobalBinConflicts({
      globalDir: ctx.globalDir,
      globalBinDir: ctx.globalBinDir,
      newPkgs: pkgs,
      shouldSkip: (pkg) => shouldReplaceExistingGlobalInstall(pkg, aliases, replacementAliases),
    })
    const retainedBinNames = await getActualBinNames({ pkgs, binsToSkip })
    const existingGlobalInstalls = await collectExistingGlobalInstalls({
      globalDir: ctx.globalDir,
      aliases,
      replacementAliases,
      retainedBinNames,
    })
    return { binsToSkip, retainedBinNames, existingGlobalInstalls }
  } catch (err) {
    return cleanupFailedGlobalInstall(installDir, err)
  }
}

async function activateGroup (
  input: GroupActivationInput & { plan: GroupActivationPlan }
): Promise<void> {
  const { ctx, installDir, aliases, pkgs, plan } = input
  const cacheHash = createGlobalCacheKey({
    aliases,
    registriesByScope: ctx.opts.registriesByScope,
  })
  const hashLink = getHashLink(ctx.globalDir, cacheHash)
  const activatedBins = await activateGlobalInstall({
    installDir,
    hashLink,
    globalBinDir: ctx.globalBinDir,
    pkgs,
    binsToSkip: plan.binsToSkip,
    requiredBinNames: plan.retainedBinNames,
  })
  await cleanupReplacedGlobalInstalls({
    groups: plan.existingGlobalInstalls.groups,
    globalDir: ctx.globalDir,
    globalBinDir: ctx.globalBinDir,
    activeHash: cacheHash,
    activatedBins,
    protectedBins: plan.existingGlobalInstalls.protectedBins,
  })
}

const PNPM_CLI_PACKAGE_ALIASES = ['pnpm', '@pnpm/exe']

export function getReplacementAliases (aliases: string[]): string[] {
  if (!aliases.some((alias) => isPnpmCliDependency(alias))) return aliases
  return [...new Set([...aliases, ...PNPM_CLI_PACKAGE_ALIASES])]
}

export function shouldReplaceExistingGlobalInstall (
  pkg: GlobalPackageInfo,
  aliases: string[],
  replacementAliases: string[]
): boolean {
  if (aliases.some((alias) => Object.hasOwn(pkg.dependencies, alias))) return true
  return isPnpmCliOnlyGroup(pkg) && replacementAliases.some((alias) => Object.hasOwn(pkg.dependencies, alias))
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

export function resolveLocalParam (param: string, baseDir: string): string {
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

interface ExistingGlobalInstalls {
  groups: GlobalPackageBinSnapshot[]
  protectedBins: Set<string>
}

async function collectExistingGlobalInstalls (
  opts: {
    globalDir: string
    aliases: string[]
    replacementAliases: string[]
    retainedBinNames: Set<string>
  }
): Promise<ExistingGlobalInstalls> {
  const { globalDir, aliases, replacementAliases, retainedBinNames } = opts

  const groupsToReplace = new Map<string, GlobalPackageInfo>()
  for (const alias of replacementAliases) {
    const existing = findGlobalPackage(globalDir, alias)
    if (
      existing &&
      shouldReplaceExistingGlobalInstall(existing, aliases, replacementAliases) &&
      !groupsToReplace.has(existing.hash)
    ) {
      groupsToReplace.set(existing.hash, existing)
    }
  }

  return getGlobalBinOwnership(globalDir, [...groupsToReplace.values()], retainedBinNames)
}
