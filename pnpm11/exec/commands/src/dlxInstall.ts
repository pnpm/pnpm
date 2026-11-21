import fs from 'node:fs'
import path from 'node:path'

import { getAutomaticallyIgnoredBuilds, rebuild } from '@pnpm/building.commands'
import type { CommandHandlerMap } from '@pnpm/cli.command'
import { isError } from '@pnpm/error'
import { add, install } from '@pnpm/installing.commands'
import { logger } from '@pnpm/logger'
import { symlinkDir } from 'symlink-dir'

import type { DlxCommandOptions } from './dlx.js'
import { type DlxCache, getPrepareDir, getValidCacheDir } from './dlxCache.js'

/**
 * Test-only env var. When set, the dlx ignored-builds recovery path bypasses
 * the TTY check and forwards `all: true` so `approve-builds` skips its
 * multiselect and confirm prompts. Mirrors the same env var honored by
 * `promptApproveGlobalBuilds` for global installs. Not for production use.
 */
const AUTO_APPROVE_FOR_TESTS_ENV = 'PNPM_AUTO_APPROVE_BUILDS_FOR_TESTS'

export interface DlxInstallContext {
  opts: DlxCommandOptions
  commands?: CommandHandlerMap
  allowBuilds: Record<string, boolean | string>
  resolvedPkgs: string[]
  resolvedOverrides?: Record<string, string>
  enableGlobalVirtualStore: boolean
}

export async function getOrPrepareDlxCache (ctx: DlxInstallContext, cache: DlxCache): Promise<string> {
  if (!cache.cacheExists) {
    return installIntoDlxCache(ctx, cache)
  }
  if (await needsDirectNonRegistryBuild(ctx, cache.cachedDir)) {
    return installIntoDlxCache(ctx, {
      cacheLink: cache.cacheLink,
      cachedDir: getPrepareDir(path.dirname(cache.cacheLink)),
      cacheExists: false,
    })
  }
  await promptApproveDlxBuilds({
    cachedDir: cache.cachedDir,
    allowBuilds: ctx.allowBuilds,
    inheritedOpts: ctx.opts,
  }, ctx.commands)
  return cache.cachedDir
}

async function needsDirectNonRegistryBuild (ctx: DlxInstallContext, dir: string): Promise<boolean> {
  if (ctx.opts.ignoreScripts) return false
  return add.hasPendingDirectNonRegistryBuilds({
    dir,
    allowBuilds: ctx.allowBuilds,
  })
}

/**
 * Installs the packages into the prepare dir of the cache and links it as the
 * current cache entry. Returns the directory the packages are installed in.
 */
async function installIntoDlxCache (ctx: DlxInstallContext, cache: DlxCache): Promise<string> {
  const { cachedDir, cacheLink } = cache
  try {
    fs.mkdirSync(cachedDir, { recursive: true })
    await add.handler(createDlxAddOptions(ctx, cachedDir), ctx.resolvedPkgs)
    await rebuildDirectNonRegistryPackages(ctx, cachedDir)
    await promptApproveDlxBuilds({ cachedDir, allowBuilds: ctx.allowBuilds, inheritedOpts: ctx.opts }, ctx.commands)
    await linkDlxCache(ctx, cachedDir, cacheLink)
  } catch (err) {
    // When parallel dlx processes install the same package, the shared global
    // virtual store can cause spurious failures (e.g. ENOENT from concurrent
    // directory swaps).  If another process completed the cache in the meantime,
    // use that instead of failing.
    const completedDir = getValidCacheDir(cacheLink, ctx.opts.dlxCacheMaxAge)
    if (completedDir != null && !await needsDirectNonRegistryBuild(ctx, completedDir)) {
      return completedDir
    }
    await removePartialDlxCache(cachedDir)
    throw err
  }
  return cachedDir
}

async function rebuildDirectNonRegistryPackages (ctx: DlxInstallContext, cachedDir: string): Promise<void> {
  if (ctx.opts.ignoreScripts) return
  const { approvedKeys, commitApprovedBuilds } = await add.approveDirectNonRegistryBuilds({
    dir: cachedDir,
    allowBuilds: ctx.allowBuilds,
  })
  if (approvedKeys.length === 0) return
  const buildOpts = createDlxAddOptions(ctx, cachedDir)
  // Global virtual store slots include build approval in their content hash, so
  // building a newly approved package stages and links a new slot via install
  // (matching `approve-builds` in `@pnpm/building.commands`).
  if (ctx.enableGlobalVirtualStore) {
    await install.handler({
      ...buildOpts,
      frozenLockfile: true,
      optimisticRepeatInstall: false,
    }, [], ctx.commands)
  } else {
    await rebuild.handler({ ...buildOpts, pending: false }, approvedKeys)
  }
  await commitApprovedBuilds()
}

function createDlxAddOptions (ctx: DlxInstallContext, cachedDir: string): add.AddCommandOptions {
  return {
    ...ctx.opts,
    allowBuild: undefined,
    overrides: ctx.resolvedOverrides,
    // Mirror the global install flow: dlx prompts via `approve-builds`
    // when transitive deps have skipped build scripts, so it must not let
    // strictDepBuilds (the v11 default) turn that into a hard error.
    // Without this, `pnpm dlx <pkg>` cannot launch packages whose bin
    // depends on a postinstall step (e.g. native modules).
    strictDepBuilds: false,
    useLockfile: true,
    enableGlobalVirtualStore: ctx.enableGlobalVirtualStore,
    bin: path.join(cachedDir, 'node_modules/.bin'),
    dir: cachedDir,
    lockfileDir: cachedDir,
    allowBuilds: ctx.allowBuilds,
    rootProjectManifestDir: cachedDir,
    saveProd: true, // dlx will be looking for the package in the "dependencies" field.
    saveDev: false,
    saveOptional: false,
    savePeer: false,
    symlink: true,
    workspaceDir: undefined,
  }
}

async function linkDlxCache (ctx: DlxInstallContext, cachedDir: string, cacheLink: string, attempt = 1): Promise<void> {
  try {
    await symlinkDir(cachedDir, cacheLink, { overwrite: true })
  } catch (error) {
    // EBUSY/EEXIST/EPERM can indicate either a parallel dlx process that acquired the cache link
    // first, or Windows briefly holding the old junction open during unlink. Yield immediately
    // when `cacheLink` already resolves to a completed, built cache; otherwise retry briefly and
    // surface the error if `cacheLink` still points at an incomplete or expired entry.
    if (!isConcurrentLinkError(error)) {
      throw error
    }
    const linkedDir = getValidCacheDir(cacheLink, ctx.opts.dlxCacheMaxAge)
    if (linkedDir != null && !await needsDirectNonRegistryBuild(ctx, linkedDir)) {
      return
    }
    if (attempt >= 5) {
      throw error
    }
    await new Promise((resolve) => setTimeout(resolve, attempt * 50))
    await linkDlxCache(ctx, cachedDir, cacheLink, attempt + 1)
  }
}

function isConcurrentLinkError (error: unknown): boolean {
  return isError(error) && 'code' in error && (error.code === 'EBUSY' || error.code === 'EEXIST' || error.code === 'EPERM')
}

/**
 * Drops the partially-populated cache so a subsequent dlx run starts
 * clean instead of reusing a broken install. This is best-effort: on
 * Windows the just-run install scripts (or antivirus) can briefly hold
 * handles on freshly written files, so retry with backoff. A cleanup
 * failure must never mask the original install error, which is the one
 * worth surfacing, so it is only logged. A leftover prepare dir is
 * harmless: it has a unique name and findCache only trusts the `pkg`
 * symlink.
 */
async function removePartialDlxCache (cachedDir: string): Promise<void> {
  try {
    await fs.promises.rm(cachedDir, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 })
  } catch (cleanupErr) {
    logger.warn({
      error: cleanupErr as Error,
      message: `Failed to clean up the dlx cache directory at "${cachedDir}"`,
      prefix: cachedDir,
    })
  }
}

/**
 * After a dlx install with `strictDepBuilds: false`, check whether any
 * transitive dependencies had their build scripts skipped and, if so, run
 * the same `approve-builds` flow that `pnpm add -g` uses. Mirrors
 * `promptApproveGlobalBuilds` in @pnpm/global.commands.
 *
 * In non-interactive mode (no TTY, no commands map) this is a no-op and
 * the install is persisted to the dlx cache with builds skipped, the same
 * behavior as `pnpm add -g` in CI. Users who need the skipped scripts
 * to run can re-invoke dlx with `--allow-build=<pkg>`, which produces a
 * different cache key and forces a fresh install.
 */
export async function promptApproveDlxBuilds (
  opts: {
    cachedDir: string
    allowBuilds: Record<string, boolean | string>
    inheritedOpts: object
  },
  commands?: CommandHandlerMap
): Promise<void> {
  if (!commands?.['approve-builds']) return
  const autoApproveForTests = process.env[AUTO_APPROVE_FOR_TESTS_ENV] === '1'
  const inCi = (opts.inheritedOpts as { ci?: boolean }).ci
  if (!autoApproveForTests && (inCi || !process.stdin.isTTY)) return
  const { automaticallyIgnoredBuilds } = await getAutomaticallyIgnoredBuilds({
    dir: opts.cachedDir,
    lockfileDir: opts.cachedDir,
  })
  const pendingBuilds = automaticallyIgnoredBuilds?.filter((pkg) => opts.allowBuilds[pkg] !== false)
  if (!pendingBuilds?.length) return
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
