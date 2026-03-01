import fs, { type Stats } from 'fs'
import path from 'path'
import util from 'util'
import { docsUrl, readProjectManifestOnly } from '@pnpm/cli-utils'
import { createResolver } from '@pnpm/client'
import { parseWantedDependency } from '@pnpm/parse-wanted-dependency'
import { OUTPUT_OPTIONS } from '@pnpm/common-cli-options-help'
import { type Config, types } from '@pnpm/config'
import { createPackageVersionPolicy } from '@pnpm/config.version-policy'
import { createHexHash } from '@pnpm/crypto.hash'
import { writeSettings } from '@pnpm/config.config-writer'
import { PnpmError } from '@pnpm/error'
import { approveBuilds } from '@pnpm/exec.build-commands'
import { installGlobalPackages, type InstallGlobalPackagesOptions } from '@pnpm/global.commands'
import { rebuild } from '@pnpm/plugin-commands-rebuild'
import { readWorkspaceManifest } from '@pnpm/workspace.read-manifest'
import { readPackageJsonFromDir } from '@pnpm/read-package-json'
import { getBinsFromPackageManifest } from '@pnpm/package-bins'
import { type PackageManifest, type PnpmSettings, type SupportedArchitectures } from '@pnpm/types'
import { lexCompare } from '@pnpm/util.lex-comparator'
import execa from 'execa'
import { pick } from 'ramda'
import renderHelp from 'render-help'
import symlinkDir from 'symlink-dir'
import { makeEnv } from './makeEnv.js'
import {
  type CatalogResolver,
  resolveFromCatalog,
} from '@pnpm/catalogs.resolver'

export const skipPackageManagerCheck = true

export const commandNames = ['dlx']

export const shorthands: Record<string, string> = {
  c: '--shell-mode',
}

export function rcOptionsTypes (): Record<string, unknown> {
  return {
    ...pick([
      'cpu',
      'libc',
      'os',
    ], types),
    'shell-mode': Boolean,
  }
}

export const cliOptionsTypes = (): Record<string, unknown> => ({
  ...rcOptionsTypes(),
  package: [String, Array],
  'allow-build': [String, Array],
})

export function help (): string {
  return renderHelp({
    description: 'Run a package in a temporary environment.',
    descriptionLists: [
      {
        title: 'Options',
        list: [
          {
            description: 'The package to install before running the command',
            name: '--package',
          },
          {
            description: 'A list of package names that are allowed to run postinstall scripts during installation',
            name: '--allow-build',
          },
          {
            description: 'Runs the script inside of a shell. Uses /bin/sh on UNIX and \\cmd.exe on Windows.',
            name: '--shell-mode',
            shortAlias: '-c',
          },
        ],
      },
      OUTPUT_OPTIONS,
    ],
    url: docsUrl('dlx'),
    usages: ['pnpm dlx <command> [args...]'],
  })
}

export type DlxCommandOptions = {
  package?: string[]
  shellMode?: boolean
  allowBuild?: string[]
} & Pick<Config, 'catalogs' | 'dlxCacheMaxAge' | 'extraBinPaths' | 'minimumReleaseAgeExclude' | 'rawLocalConfig' | 'reporter' | 'supportedArchitectures' | 'symlink'> & InstallGlobalPackagesOptions & PnpmSettings

export async function handler (
  opts: DlxCommandOptions,
  [command, ...args]: string[]
): Promise<{ exitCode: number, output?: string }> {
  if (!command && (!opts.package || opts.package.length === 0)) {
    return { exitCode: 1, output: help() }
  }
  const pkgs = opts.package ?? [command]
  const fullMetadata = (
    (
      opts.resolutionMode === 'time-based' ||
      Boolean(opts.minimumReleaseAge) ||
      opts.trustPolicy === 'no-downgrade'
    ) && !opts.registrySupportsTimeField
  )
  const catalogResolver = resolveFromCatalog.bind(null, opts.catalogs ?? {})
  const { resolve } = createResolver({
    ...opts,
    authConfig: opts.rawConfig,
    fullMetadata,
    filterMetadata: fullMetadata,
    retry: {
      factor: opts.fetchRetryFactor,
      maxTimeout: opts.fetchRetryMaxtimeout,
      minTimeout: opts.fetchRetryMintimeout,
      retries: opts.fetchRetries,
    },
    timeout: opts.fetchTimeout,
  })
  const resolvedPkgAliases: string[] = []
  const publishedBy = opts.minimumReleaseAge ? new Date(Date.now() - opts.minimumReleaseAge * 60 * 1000) : undefined
  const publishedByExclude = opts.minimumReleaseAgeExclude
    ? createPackageVersionPolicy(opts.minimumReleaseAgeExclude)
    : undefined
  const resolvedPkgs = await Promise.all(pkgs.map(async (pkg) => {
    const { alias, bareSpecifier } = parseWantedDependency(pkg) || {}
    if (alias == null) return pkg
    const resolvedBareSpecifier = bareSpecifier != null
      ? resolveCatalogProtocol(catalogResolver, alias, bareSpecifier)
      : bareSpecifier
    resolvedPkgAliases.push(alias)
    const resolved = await resolve({ alias, bareSpecifier: resolvedBareSpecifier }, {
      lockfileDir: opts.lockfileDir ?? opts.dir,
      preferredVersions: {},
      projectDir: opts.dir,
      publishedBy,
      publishedByExclude,
    })
    return resolved.id
  }))
  let { cacheLink, cacheExists, cachedDir } = findCache({
    packages: resolvedPkgs,
    dlxCacheMaxAge: opts.dlxCacheMaxAge,
    cacheDir: opts.cacheDir,
    registries: opts.registries,
    supportedArchitectures: opts.supportedArchitectures,
  })
  const allowBuilds = buildAllowBuildsRecord(resolvedPkgAliases, opts.allowBuild)
  if (!cacheExists) {
    cachedDir = await installAndApproveBuilds(opts, cachedDir, resolvedPkgs, allowBuilds)
    await tryCacheLink(cachedDir, cacheLink)
  } else {
    const cachedAllowBuilds = await readCachedAllowBuilds(cachedDir)
    const delta = computeAllowBuildsDelta(allowBuilds, cachedAllowBuilds)
    if (delta.action === 'invalidate') {
      const dlxCommandCacheDir = createDlxCommandCacheDir({
        packages: resolvedPkgs,
        registries: opts.registries,
        cacheDir: opts.cacheDir,
        supportedArchitectures: opts.supportedArchitectures,
      })
      cachedDir = await installAndApproveBuilds(opts, getPrepareDir(dlxCommandCacheDir), resolvedPkgs, allowBuilds)
      await tryCacheLink(cachedDir, cacheLink)
    } else if (delta.action === 'rebuild') {
      await rebuild.handler({
        ...opts,
        dir: cachedDir,
        lockfileDir: cachedDir,
        rootProjectManifest: undefined,
        rootProjectManifestDir: cachedDir,
        workspaceDir: cachedDir,
        pending: false,
        allowBuilds,
      } as Parameters<typeof rebuild.handler>[0], delta.newlyAllowed)
      await persistAllowBuilds(cachedDir, allowBuilds)
    }
  }
  const binsDir = path.join(cachedDir, 'node_modules/.bin')
  const env = makeEnv({
    userAgent: opts.userAgent,
    prependPaths: [binsDir, ...opts.extraBinPaths],
  })
  const binName = opts.package
    ? command
    : await getBinName(cachedDir, opts)
  try {
    await execa(binName, args, {
      cwd: process.cwd(),
      env,
      stdio: 'inherit',
      shell: opts.shellMode ?? false,
    })
  } catch (err: unknown) {
    if (util.types.isNativeError(err) && 'exitCode' in err && err.exitCode != null) {
      return {
        exitCode: err.exitCode as number,
      }
    }
    throw err
  }
  return { exitCode: 0 }
}

async function getPkgName (pkgDir: string): Promise<string> {
  const manifest = await readPackageJsonFromDir(pkgDir)
  const dependencyNames = Object.keys(manifest.dependencies ?? {})
  if (dependencyNames.length === 0) {
    throw new PnpmError('DLX_NO_DEP', 'dlx was unable to find the installed dependency in "dependencies"')
  }
  return dependencyNames[0]
}

async function getBinName (cachedDir: string, opts: Pick<DlxCommandOptions, 'engineStrict'>): Promise<string> {
  const pkgName = await getPkgName(cachedDir)
  const pkgDir = path.join(cachedDir, 'node_modules', pkgName)
  const manifest = await readProjectManifestOnly(pkgDir, opts) as PackageManifest
  const bins = await getBinsFromPackageManifest(manifest, pkgDir)
  if (bins.length === 0) {
    throw new PnpmError('DLX_NO_BIN', `No binaries found in ${pkgName}`)
  }
  if (bins.length === 1) {
    return bins[0].name
  }
  const scopelessPkgName = scopeless(manifest.name)
  const defaultBin = bins.find(({ name }) => name === scopelessPkgName)
  if (defaultBin) return defaultBin.name
  const binNames = bins.map(({ name }) => name)
  throw new PnpmError('DLX_MULTIPLE_BINS', `Could not determine executable to run. ${pkgName} has multiple binaries: ${binNames.join(', ')}`, {
    hint: `Try one of the following:
${binNames.map(name => `pnpm --package=${pkgName} dlx ${name}`).join('\n')}
`,
  })
}

function scopeless (pkgName: string): string {
  if (pkgName[0] === '@') {
    return pkgName.split('/')[1]
  }
  return pkgName
}

function findCache (opts: {
  packages: string[]
  cacheDir: string
  dlxCacheMaxAge: number
  registries: Record<string, string>
  supportedArchitectures?: SupportedArchitectures
}): { cacheLink: string, cacheExists: boolean, cachedDir: string } {
  const dlxCommandCacheDir = createDlxCommandCacheDir(opts)
  const cacheLink = path.join(dlxCommandCacheDir, 'pkg')
  const cachedDir = getValidCacheDir(cacheLink, opts.dlxCacheMaxAge)
  return {
    cacheLink,
    cachedDir: cachedDir ?? getPrepareDir(dlxCommandCacheDir),
    cacheExists: cachedDir != null,
  }
}

function createDlxCommandCacheDir (
  opts: {
    packages: string[]
    registries: Record<string, string>
    cacheDir: string
    supportedArchitectures?: SupportedArchitectures
  }
): string {
  const dlxCacheDir = path.resolve(opts.cacheDir, 'dlx')
  const cacheKey = createCacheKey(opts)
  const cachePath = path.join(dlxCacheDir, cacheKey)
  fs.mkdirSync(cachePath, { recursive: true })
  return cachePath
}

export function createCacheKey (opts: {
  packages: string[]
  registries: Record<string, string>
  supportedArchitectures?: SupportedArchitectures
}): string {
  const sortedPkgs = [...opts.packages].sort(lexCompare)
  const sortedRegistries = Object.entries(opts.registries).sort(([k1], [k2]) => lexCompare(k1, k2))
  const args: unknown[] = [sortedPkgs, sortedRegistries]
  if (opts.supportedArchitectures) {
    const supportedArchitecturesKeys = ['cpu', 'libc', 'os'] as const satisfies Array<keyof SupportedArchitectures>
    for (const key of supportedArchitecturesKeys) {
      const value = opts.supportedArchitectures[key]
      if (!value?.length) continue
      args.push({
        supportedArchitectures: {
          [key]: [...new Set(value)].sort(lexCompare),
        },
      })
    }
  }
  const hashStr = JSON.stringify(args)
  return createHexHash(hashStr)
}

function getValidCacheDir (cacheLink: string, dlxCacheMaxAge: number): string | undefined {
  let stats: Stats
  let target: string
  try {
    stats = fs.lstatSync(cacheLink)
    if (stats.isSymbolicLink()) {
      target = fs.realpathSync(cacheLink)
      if (!target) return undefined
    } else {
      return undefined
    }
  } catch (err) {
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ENOENT') {
      return undefined
    }
    throw err
  }
  const isValid = stats.mtime.getTime() + dlxCacheMaxAge * 60_000 >= new Date().getTime()
  return isValid ? target : undefined
}

function getPrepareDir (cachePath: string): string {
  const name = `${new Date().getTime().toString(16)}-${process.pid.toString(16)}`
  return path.join(cachePath, name)
}

function buildAllowBuildsRecord (
  resolvedPkgAliases: string[],
  allowBuild?: string[]
): Record<string, boolean | string> {
  return Object.fromEntries(
    [...resolvedPkgAliases, ...(allowBuild ?? [])].map((pkg: string) => [pkg, true])
  )
}

async function installAndApproveBuilds (
  opts: DlxCommandOptions,
  targetDir: string,
  resolvedPkgs: string[],
  allowBuilds: Record<string, boolean | string>
): Promise<string> {
  fs.mkdirSync(targetDir, { recursive: true })
  const ignoredBuilds = await installGlobalPackages({
    ...opts,
    bin: path.join(targetDir, 'node_modules/.bin'),
    dir: targetDir,
    lockfileDir: targetDir,
    allowBuilds,
    include: {
      dependencies: true,
      devDependencies: false,
      optionalDependencies: true,
    },
    symlink: true,
  }, resolvedPkgs)
  if (ignoredBuilds?.size && process.stdin.isTTY) {
    await approveBuilds.handler({
      ...opts,
      modulesDir: path.join(targetDir, 'node_modules'),
      dir: targetDir,
      lockfileDir: targetDir,
      rootProjectManifest: undefined,
      rootProjectManifestDir: targetDir,
      workspaceDir: targetDir,
      global: false,
      pending: false,
      allowBuilds,
    } as Parameters<typeof approveBuilds.handler>[0])
  } else {
    await persistAllowBuilds(targetDir, allowBuilds)
  }
  return targetDir
}

async function tryCacheLink (cachedDir: string, cacheLink: string): Promise<void> {
  try {
    await symlinkDir(cachedDir, cacheLink, { overwrite: true })
  } catch (error) {
    // EBUSY means that there is another dlx process running in parallel that has acquired the cache link first.
    // Similarly, EEXIST means that another dlx process has created the cache link before this process.
    // The link created by the other process is just as up-to-date as the link the current process was attempting
    // to create. Therefore, instead of re-attempting to create the current link again, it is just as good to let
    // the other link stay. The current process should yield.
    if (!util.types.isNativeError(error) || !('code' in error) || (error.code !== 'EBUSY' && error.code !== 'EEXIST')) {
      throw error
    }
  }
}

async function persistAllowBuilds (
  dir: string,
  allowBuilds: Record<string, boolean | string>
): Promise<void> {
  await writeSettings({
    rootProjectManifest: undefined,
    rootProjectManifestDir: dir,
    workspaceDir: dir,
    updatedSettings: { allowBuilds },
  })
}

async function readCachedAllowBuilds (cachedDir: string): Promise<Record<string, boolean | string>> {
  try {
    const manifest = await readWorkspaceManifest(cachedDir)
    return manifest?.allowBuilds ?? {}
  } catch {
    return {}
  }
}

export interface AllowBuildsDelta {
  action: 'none' | 'rebuild' | 'invalidate'
  newlyAllowed: string[]
}

export function computeAllowBuildsDelta (
  currentAllowBuilds: Record<string, boolean | string>,
  cachedAllowBuilds: Record<string, boolean | string>
): AllowBuildsDelta {
  // If a previously built package is no longer allowed, we must invalidate
  // because we cannot "un-build" it.
  for (const [pkg, wasAllowed] of Object.entries(cachedAllowBuilds)) {
    if (wasAllowed === true && currentAllowBuilds[pkg] !== true) {
      return { action: 'invalidate', newlyAllowed: [] }
    }
  }
  // Check for newly allowed packages that weren't built before
  const newlyAllowed: string[] = []
  for (const [pkg, isAllowed] of Object.entries(currentAllowBuilds)) {
    if (isAllowed === true && cachedAllowBuilds[pkg] !== true) {
      newlyAllowed.push(pkg)
    }
  }
  if (newlyAllowed.length === 0) {
    return { action: 'none', newlyAllowed: [] }
  }
  return { action: 'rebuild', newlyAllowed }
}

function resolveCatalogProtocol (catalogResolver: CatalogResolver, alias: string, bareSpecifier: string): string {
  const result = catalogResolver({ alias, bareSpecifier })

  switch (result.type) {
  case 'found': return result.resolution.specifier
  case 'unused': return bareSpecifier
  case 'misconfiguration': throw result.error
  }
}
