import path from 'node:path'

import { getBinsFromPackageManifest } from '@pnpm/bins.resolver'
import {
  type CatalogResolver,
  resolveFromCatalog,
} from '@pnpm/catalogs.resolver'
import type { CommandHandlerMap } from '@pnpm/cli.command'
import { OPTIONS, OUTPUT_OPTIONS } from '@pnpm/cli.common-cli-options-help'
import { docsUrl, readProjectManifestOnly } from '@pnpm/cli.utils'
import { type Config, type ConfigContext, types } from '@pnpm/config.reader'
import { getPublishedByPolicy, MINIMUM_RELEASE_AGE_OPTION_TYPES } from '@pnpm/config.version-policy'
import { getSystemNodeVersion } from '@pnpm/engine.runtime.system-version'
import { isError, PnpmError } from '@pnpm/error'
import { addEsmNodePathLoaderOption } from '@pnpm/exec.esm-node-path-loader'
import { createResolver, makeResolutionStrict } from '@pnpm/installing.client'
import { add } from '@pnpm/installing.commands'
import { readPackageJsonFromDir } from '@pnpm/pkg-manifest.reader'
import { parseWantedDependency } from '@pnpm/resolving.parse-wanted-dependency'
import type { PackageManifest, PnpmSettings } from '@pnpm/types'
import { pick } from 'ramda'
import { renderHelp } from 'render-help'

import { createCacheKey, type DlxCache, findCache, resolveDlxOverrides } from './dlxCache.js'
import { getOrPrepareDlxCache } from './dlxInstall.js'
import { makeEnv } from './makeEnv.js'
import { trackedExeca, waitForTracked } from './trackedExeca.js'

export { createCacheKey }

export const skipPackageManagerCheck = true

export const commandNames = ['dlx']

export const shorthands: Record<string, string> = {
  c: '--shell-mode',
}

export function rcOptionsTypes (): Record<string, unknown> {
  return {
    ...pick([
      'cpu',
      'ignore-scripts',
      'libc',
      'os',
    ], types),
    ...MINIMUM_RELEASE_AGE_OPTION_TYPES,
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
            description: 'A list of package names that are allowed to run postinstall scripts during installation. Prefix a name with ! to deny its scripts instead',
            name: '--allow-build',
          },
          {
            description: 'Runs the script inside of a shell. Uses /bin/sh on UNIX and \\cmd.exe on Windows.',
            name: '--shell-mode',
            shortAlias: '-c',
          },
          OPTIONS.minimumReleaseAge,
          OPTIONS.minimumReleaseAgeExclude,
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
} & Pick<Config, 'extraBinPaths' | 'minimumReleaseAgeExclude' | 'registriesByScope' | 'reporter' | 'userAgent' | 'cacheDir' | 'dlxCacheMaxAge' | 'symlink'> & Partial<Pick<Config, 'blockExoticSubdeps'>> & Partial<Pick<ConfigContext, 'cliOptions'>> & Omit<add.AddCommandOptions, 'rootProjectManifestDir'> & PnpmSettings

export async function handler (
  opts: DlxCommandOptions,
  [command, ...args]: string[],
  commands?: CommandHandlerMap
): Promise<{ exitCode: number, output?: string }> {
  if (!command) {
    if (!opts.package?.length) {
      return { exitCode: 1, output: help() }
    }
    throw new PnpmError('DLX_MISSING_COMMAND', "'pnpm dlx' requires a command to run")
  }
  const invocationDir = (opts.cliOptions?.dir as string | undefined) ?? process.cwd()
  const allowBuildSelectors = add.parseAllowBuildSelectors({ allowBuild: opts.allowBuild })
  const resolvedOverrides = resolveDlxOverrides(opts.overrides, opts.catalogs)
  const { resolvedPkgs, resolvedPkgAliases } = await resolveDlxPackages(
    opts,
    (opts.package ?? [command]).map((pkg) => add.resolveLocalPackageSpec(pkg, invocationDir))
  )
  const enableGlobalVirtualStore = opts.enableGlobalVirtualStore ?? true
  const cache = findDlxCache(opts, resolvedPkgs, resolvedOverrides)
  const allowBuilds = add.applyAllowBuildSelectors(
    Object.fromEntries(resolvedPkgAliases.map(pkg => [pkg, true])),
    allowBuildSelectors
  )
  const cachedDir = await getOrPrepareDlxCache({
    opts,
    commands,
    allowBuilds,
    resolvedPkgs,
    resolvedOverrides,
    enableGlobalVirtualStore,
  }, cache)
  const env = createDlxEnv(opts, cachedDir, enableGlobalVirtualStore)
  const binName = opts.package
    ? command
    : await getBinName(cachedDir, opts)
  return runDlxBin(binName, args, {
    cwd: invocationDir,
    env,
    shellMode: opts.shellMode,
  })
}

function findDlxCache (
  opts: DlxCommandOptions,
  packages: string[],
  overrides: Record<string, string> | undefined
): DlxCache {
  return findCache({
    packages,
    dlxCacheMaxAge: opts.dlxCacheMaxAge,
    cacheDir: opts.cacheDir,
    registriesByScope: opts.registriesByScope,
    allowBuild: opts.allowBuild,
    supportedArchitectures: opts.supportedArchitectures,
    nodeVersion: getSystemNodeVersion(),
    overrides,
    packageExtensions: opts.packageExtensions,
    minimumReleaseAge: opts.minimumReleaseAge,
    minimumReleaseAgeExclude: opts.minimumReleaseAgeExclude,
    minimumReleaseAgeIgnoreMissingTime: opts.minimumReleaseAgeIgnoreMissingTime,
    minimumReleaseAgeStrict: opts.minimumReleaseAgeStrict,
    trustPolicy: opts.trustPolicy,
    trustPolicyExclude: opts.trustPolicyExclude,
    trustPolicyIgnoreAfter: opts.trustPolicyIgnoreAfter,
    trustLockfile: opts.trustLockfile,
    blockExoticSubdeps: opts.blockExoticSubdeps,
    ignoreScripts: opts.ignoreScripts,
  })
}

type DlxResolve = ReturnType<typeof createResolver>['resolve']

async function resolveDlxPackages (
  opts: DlxCommandOptions,
  pkgs: string[]
): Promise<{ resolvedPkgs: string[], resolvedPkgAliases: string[] }> {
  const catalogResolver = resolveFromCatalog.bind(null, opts.catalogs ?? {})
  const resolve = createDlxResolver(opts)
  const resolvedPkgAliases: string[] = []
  const { publishedBy, publishedByExclude } = getPublishedByPolicy(opts)
  const resolvedPkgs = await Promise.all(pkgs.map(async (pkg) => {
    const { alias, bareSpecifier } = parseWantedDependency(pkg) || {}
    if (alias == null) return pkg
    const resolvedBareSpecifier = bareSpecifier != null
      ? resolveCatalogProtocol(catalogResolver, alias, bareSpecifier)
      : bareSpecifier
    resolvedPkgAliases.push(alias)
    if (resolvedBareSpecifier != null && add.isLocalFilesystemSpecifier(resolvedBareSpecifier)) {
      return `${alias}@${resolvedBareSpecifier}`
    }
    const resolved = await resolve({ alias, bareSpecifier: resolvedBareSpecifier }, {
      lockfileDir: opts.lockfileDir ?? opts.dir,
      preferredVersions: {},
      projectDir: opts.dir,
      publishedBy,
      publishedByExclude,
    })
    return resolved.id
  }))
  return { resolvedPkgs, resolvedPkgAliases }
}

function createDlxResolver (opts: DlxCommandOptions): DlxResolve {
  const fullMetadata = (
    (
      opts.resolutionMode === 'time-based' ||
      opts.trustPolicy === 'no-downgrade' ||
      Boolean(opts.minimumReleaseAge)
    ) && !opts.registrySupportsTimeField
  )
  const { resolve: baseResolve } = createResolver({
    ...opts,
    configByUri: opts.configByUri,
    fullMetadata,
    filterMetadata: fullMetadata,
    ignoreMissingTimeField: opts.minimumReleaseAgeIgnoreMissingTime,
    retry: {
      factor: opts.fetchRetryFactor,
      maxTimeout: opts.fetchRetryMaxtimeout,
      minTimeout: opts.fetchRetryMintimeout,
      retries: opts.fetchRetries,
    },
    timeout: opts.fetchTimeout,
  })
  // dlx has nowhere to "defer to" — it runs the resolved package directly.
  // Wrap the resolver under any policy that wants to reject violations
  // up-front: strict minimumReleaseAge (refuse immature picks) and
  // `trustPolicy: 'no-downgrade'` (refuse versions whose trust evidence
  // weakened). Without the trust-policy arm, a downgraded version would
  // resolve to a `policyViolation` that dlx silently ignored and then
  // executed.
  const strictResolution =
    (Boolean(opts.minimumReleaseAge) && opts.minimumReleaseAgeStrict === true) ||
    opts.trustPolicy === 'no-downgrade'
  return strictResolution ? makeResolutionStrict(baseResolve) : baseResolve
}

function createDlxEnv (opts: DlxCommandOptions, cachedDir: string, enableGlobalVirtualStore: boolean): NodeJS.ProcessEnv {
  const binsDir = path.join(cachedDir, 'node_modules/.bin')
  return makeEnv({
    userAgent: opts.userAgent,
    prependPaths: [binsDir, ...opts.extraBinPaths],
    // The bin's command shim provides NODE_PATH; the loader makes those
    // lookups work for ESM imports too.
    extraEnv: enableGlobalVirtualStore
      ? { NODE_OPTIONS: addEsmNodePathLoaderOption(process.env.NODE_OPTIONS) }
      : {},
  })
}

async function runDlxBin (
  binName: string,
  args: string[],
  opts: { cwd: string, env: NodeJS.ProcessEnv, shellMode?: boolean }
): Promise<{ exitCode: number }> {
  try {
    const child = trackedExeca(binName, args, {
      cwd: opts.cwd,
      env: opts.env,
      stdio: 'inherit',
      shell: opts.shellMode ?? false,
    })
    await waitForTracked(child)
  } catch (err: unknown) {
    if (isError(err) && 'exitCode' in err && err.exitCode != null) {
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
  let manifest: PackageManifest
  try {
    manifest = await readProjectManifestOnly(pkgDir, opts) as PackageManifest
  } catch (err: unknown) {
    // The installed package's `package.json` is unreadable. Observed in the
    // wild for `node@runtime:<version>` whose CAS slot was materialized by
    // a code path that didn't run pnpm's `appendManifest`, leaving the slot without
    // the `package.json` runtime archives don't ship themselves. Fall back
    // to the scopeless package name — for single-bin packages (the dlx
    // common case) it matches what `manifest.bin` would have named, and
    // the `node_modules/.bin/<name>` symlink the install already wired up
    // from the resolution's bin info is what `execa` resolves against.
    // Multi-bin packages require `--package=<spec> <bin>` to disambiguate,
    // which short-circuits `getBinName` upstream and never enters this path.
    if (isError(err) && 'code' in err && err.code === 'ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND') {
      return scopeless(pkgName)
    }
    throw err
  }
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

function resolveCatalogProtocol (catalogResolver: CatalogResolver, alias: string, bareSpecifier: string): string {
  const result = catalogResolver({ alias, bareSpecifier })

  switch (result.type) {
    case 'found': return result.resolution.specifier
    case 'unused': return bareSpecifier
    case 'misconfiguration': throw result.error
  }
}
