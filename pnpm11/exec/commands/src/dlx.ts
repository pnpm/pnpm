import fs, { type Stats } from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { getBinsFromPackageManifest } from '@pnpm/bins.resolver'
import { getAutomaticallyIgnoredBuilds } from '@pnpm/building.commands'
import {
  type CatalogResolver,
  resolveFromCatalog,
} from '@pnpm/catalogs.resolver'
import type { CommandHandlerMap } from '@pnpm/cli.command'
import { OUTPUT_OPTIONS } from '@pnpm/cli.common-cli-options-help'
import { docsUrl, readProjectManifestOnly } from '@pnpm/cli.utils'
import { type Config, types } from '@pnpm/config.reader'
import { getPublishedByPolicy } from '@pnpm/config.version-policy'
import { createShortHash } from '@pnpm/crypto.hash'
import { PnpmError } from '@pnpm/error'
import { createResolver, makeResolutionStrict } from '@pnpm/installing.client'
import { add } from '@pnpm/installing.commands'
import { logger } from '@pnpm/logger'
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

/**
 * Test-only env var. When set, the dlx ignored-builds recovery path bypasses
 * the TTY check and forwards `all: true` so `approve-builds` skips its
 * multiselect and confirm prompts. Mirrors the same env var honored by
 * `promptApproveGlobalBuilds` for global installs. Not for production use.
 */
const AUTO_APPROVE_FOR_TESTS_ENV = 'PNPM_AUTO_APPROVE_BUILDS_FOR_TESTS'

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
  [command, ...args]: string[],
  commands?: CommandHandlerMap
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
  const resolve = strictResolution ? makeResolutionStrict(baseResolve) : baseResolve
  const resolvedPkgAliases: string[] = []
  const { publishedBy, publishedByExclude } = getPublishedByPolicy(opts)
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
    const allowBuilds = Object.fromEntries([...resolvedPkgAliases, ...(opts.allowBuild ?? [])].map(pkg => [pkg, true]))
    try {
      fs.mkdirSync(cachedDir, { recursive: true })
      await add.handler({
        ...opts,
        // Mirror the global install flow: dlx prompts via `approve-builds`
        // when transitive deps have skipped build scripts, so it must not let
        // strictDepBuilds (the v11 default) turn that into a hard error.
        // Without this, `pnpm dlx <pkg>` cannot launch packages whose bin
        // depends on a postinstall step (e.g. native modules).
        strictDepBuilds: false,
        enableGlobalVirtualStore: opts.enableGlobalVirtualStore ?? true,
        bin: path.join(cachedDir, 'node_modules/.bin'),
        dir: cachedDir,
        lockfileDir: cachedDir,
        allowBuilds,
        rootProjectManifestDir: cachedDir,
        saveProd: true, // dlx will be looking for the package in the "dependencies" field!
        saveDev: false,
        saveOptional: false,
        savePeer: false,
        symlink: true,
        workspaceDir: undefined,
      }, resolvedPkgs)
      await promptApproveDlxBuilds({ cachedDir, allowBuilds, inheritedOpts: opts }, commands)
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
      if (completedDir != null) {
        cachedDir = completedDir
      } else {
        // Drop the partially-populated cache so a subsequent dlx run starts
        // clean instead of reusing a broken install. This is best-effort: on
        // Windows the just-run install scripts (or antivirus) can briefly hold
        // handles on freshly written files, so retry with backoff. A cleanup
        // failure must never mask the original install error, which is the one
        // worth surfacing — log it and rethrow err. A leftover prepare dir is
        // harmless: it has a unique name and findCache only trusts the `pkg`
        // symlink.
        try {
          await fs.promises.rm(cachedDir, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 })
        } catch (cleanupErr) {
          logger.warn({
            error: cleanupErr as Error,
            message: `Failed to clean up the dlx cache directory at "${cachedDir}"`,
            prefix: cachedDir,
          })
        }
        throw err
      }
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
  let manifest: PackageManifest
  try {
    manifest = await readProjectManifestOnly(pkgDir, opts) as PackageManifest
  } catch (err: unknown) {
    // The installed package's `package.json` is unreadable. Observed in the
    // wild for `node@runtime:<version>` whose CAS slot was materialized by
    // a code path that didn't run pnpm's `appendManifest` (or pacquet's
    // equivalent runtime-manifest synthesis), leaving the slot without
    // the `package.json` runtime archives don't ship themselves. Fall back
    // to the scopeless package name — for single-bin packages (the dlx
    // common case) it matches what `manifest.bin` would have named, and
    // the `node_modules/.bin/<name>` symlink the install already wired up
    // from the resolution's bin info is what `execa` resolves against.
    // Multi-bin packages require `--package=<spec> <bin>` to disambiguate,
    // which short-circuits `getBinName` upstream and never enters this path.
    if (util.types.isNativeError(err) && 'code' in err && err.code === 'ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND') {
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

/**
 * After a dlx install with `strictDepBuilds: false`, check whether any
 * transitive dependencies had their build scripts skipped and, if so, run
 * the same `approve-builds` flow that `pnpm add -g` uses. Mirrors
 * `promptApproveGlobalBuilds` in @pnpm/global.commands.
 *
 * In non-interactive mode (no TTY, no commands map) this is a no-op and
 * the install is persisted to the dlx cache with builds skipped — same
 * behavior as `pnpm add -g` in CI. Users who need the skipped scripts
 * to run can re-invoke dlx with `--allow-build=<pkg>`, which produces a
 * different cache key and forces a fresh install.
 */
async function promptApproveDlxBuilds (
  opts: {
    cachedDir: string
    allowBuilds: Record<string, boolean | string>
    inheritedOpts: object
  },
  commands?: CommandHandlerMap
): Promise<void> {
  if (!commands?.['approve-builds']) return
  const autoApproveForTests = process.env[AUTO_APPROVE_FOR_TESTS_ENV] === '1'
  if (!autoApproveForTests && !process.stdin.isTTY) return
  const { automaticallyIgnoredBuilds } = await getAutomaticallyIgnoredBuilds({
    dir: opts.cachedDir,
    lockfileDir: opts.cachedDir,
  })
  if (!automaticallyIgnoredBuilds?.length) return
  await commands['approve-builds']({
    ...opts.inheritedOpts,
    dir: opts.cachedDir,
    lockfileDir: opts.cachedDir,
    rootProjectManifestDir: opts.cachedDir,
    modulesDir: undefined,
    workspaceDir: undefined,
    allProjects: undefined,
    selectedProjectsGraph: undefined,
    workspacePackagePatterns: undefined,
    rootProjectManifest: undefined,
    global: false,
    pending: false,
    allowBuilds: opts.allowBuilds,
    all: autoApproveForTests ? true : undefined,
  }, [], commands)
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
  // A short (truncated) hash keeps the dlx cache path short. The full
  // virtual-store path below it (`<key>/<prepare>/node_modules/.pnpm/<pkgId>/
  // node_modules/<pkg>`) can otherwise blow past Windows' MAX_PATH (260) and
  // make lifecycle scripts fail with a `spawn cmd.exe ENOENT` (the cwd no
  // longer resolves). 128 bits is ample collision resistance for a cache key.
  return createShortHash(hashStr)
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
  // base36 (vs hex) keeps this segment short — see createCacheKey for why dlx
  // path length matters on Windows. time+pid stays unique across concurrent
  // dlx processes and across a process's own retries of a failed install.
  const name = `${Date.now().toString(36)}-${process.pid.toString(36)}`
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
