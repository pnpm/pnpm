import fs, { type Stats } from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { getBinsFromPackageManifest } from '@pnpm/bins.resolver'
import {
  type CatalogResolver,
  resolveFromCatalog,
} from '@pnpm/catalogs.resolver'
import { OUTPUT_OPTIONS } from '@pnpm/cli.common-cli-options-help'
import { docsUrl, readProjectManifestOnly } from '@pnpm/cli.utils'
import { type Config, types } from '@pnpm/config.reader'
import { createPackageVersionPolicy } from '@pnpm/config.version-policy'
import { createHexHash } from '@pnpm/crypto.hash'
import { PnpmError } from '@pnpm/error'
import { createResolver } from '@pnpm/installing.client'
import { add } from '@pnpm/installing.commands'
import { readPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import { parseWantedDependency } from '@pnpm/resolving.parse-wanted-dependency'
import type { PackageManifest, PnpmSettings, SupportedArchitectures } from '@pnpm/types'
import { lexCompare } from '@pnpm/util.lex-comparator'
import { safeExeca as execa } from 'execa'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'
import { symlinkDir } from 'symlink-dir'

import { makeEnv } from './makeEnv.js'

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
} & Pick<Config, 'extraBinPaths' | 'minimumReleaseAgeExclude' | 'registries' | 'reporter' | 'userAgent' | 'cacheDir' | 'dlxCacheMaxAge' | 'symlink'> & Omit<add.AddCommandOptions, 'rootProjectManifestDir'> & PnpmSettings

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
      opts.trustPolicy === 'no-downgrade' ||
      Boolean(opts.minimumReleaseAge)
    ) && !opts.registrySupportsTimeField
  )
  const catalogResolver = resolveFromCatalog.bind(null, opts.catalogs ?? {})
  const { resolve } = createResolver({
    ...opts,
    configByUri: opts.configByUri,
    fullMetadata,
    filterMetadata: fullMetadata,
    strictPublishedByCheck: Boolean(opts.minimumReleaseAge) && opts.minimumReleaseAgeStrict === true,
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
    allowBuild: opts.allowBuild,
    supportedArchitectures: opts.supportedArchitectures,
  })
  if (!cacheExists) {
    try {
      fs.mkdirSync(cachedDir, { recursive: true })
      await add.handler({
        ...opts,
        enableGlobalVirtualStore: opts.enableGlobalVirtualStore ?? true,
        bin: path.join(cachedDir, 'node_modules/.bin'),
        dir: cachedDir,
        lockfileDir: cachedDir,
        allowBuilds: Object.fromEntries([...resolvedPkgAliases, ...(opts.allowBuild ?? [])].map(pkg => [pkg, true])),
        rootProjectManifestDir: cachedDir,
        saveProd: true, // dlx will be looking for the package in the "dependencies" field!
        saveDev: false,
        saveOptional: false,
        savePeer: false,
        symlink: true,
        workspaceDir: undefined,
      }, resolvedPkgs)
      try {
        await symlinkDir(cachedDir, cacheLink, { overwrite: true })
      } catch (error) {
        // EBUSY/EEXIST/EPERM means that there is another dlx process running in parallel that has acquired the cache link first.
        // EPERM can happen on Windows when another process has the symlink open while this process tries to unlink it.
        // The link created by the other process is just as up-to-date as the link the current process was attempting
        // to create. Therefore, instead of re-attempting to create the current link again, it is just as good to let
        // the other link stay. The current process should yield.
        if (!util.types.isNativeError(error) || !('code' in error) || (error.code !== 'EBUSY' && error.code !== 'EEXIST' && error.code !== 'EPERM')) {
          throw error
        }
      }
    } catch (err) {
      // When parallel dlx processes install the same package, the shared global
      // virtual store can cause spurious failures (e.g. ENOENT from concurrent
      // directory swaps).  If another process completed the cache in the meantime,
      // use that instead of failing.
      const completedDir = getValidCacheDir(cacheLink, opts.dlxCacheMaxAge)
      if (completedDir == null) {
        throw err
      }
      cachedDir = completedDir
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
  allowBuild?: string[]
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
    allowBuild?: string[]
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
  allowBuild?: string[]
  supportedArchitectures?: SupportedArchitectures
}): string {
  const sortedPkgs = [...opts.packages].sort(lexCompare)
  const sortedRegistries = Object.entries(opts.registries).sort(([k1], [k2]) => lexCompare(k1, k2))
  const args: unknown[] = [sortedPkgs, sortedRegistries]
  if (opts.allowBuild?.length) {
    args.push({ allowBuild: opts.allowBuild.sort(lexCompare) })
  }
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

function resolveCatalogProtocol (catalogResolver: CatalogResolver, alias: string, bareSpecifier: string): string {
  const result = catalogResolver({ alias, bareSpecifier })

  switch (result.type) {
    case 'found': return result.resolution.specifier
    case 'unused': return bareSpecifier
    case 'misconfiguration': throw result.error
  }
}
