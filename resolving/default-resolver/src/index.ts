import { type BunRuntimeResolveResult, resolveBunRuntime } from '@pnpm/engine.runtime.bun-resolver'
import { type DenoRuntimeResolveResult, resolveDenoRuntime } from '@pnpm/engine.runtime.deno-resolver'
import { type NodeRuntimeResolveResult, resolveNodeRuntime } from '@pnpm/engine.runtime.node-resolver'
import { PnpmError } from '@pnpm/error'
import type { FetchFromRegistry, GetAuthHeader } from '@pnpm/fetching.types'
import { checkCustomResolverCanResolve, type CustomResolver } from '@pnpm/hooks.types'
import { createGitResolver, type GitResolveResult } from '@pnpm/resolving.git-resolver'
import { type LocalResolveResult, resolveFromLocal } from '@pnpm/resolving.local-resolver'
import {
  createNpmResolver,
  type JsrResolveResult,
  type NpmResolveResult,
  type PackageMeta,
  type PackageMetaCache,
  type ResolveFromNpmOptions,
  type ResolverFactoryOptions,
  type WorkspaceResolveResult,
} from '@pnpm/resolving.npm-resolver'
import type {
  ResolveFunction,
  ResolveOptions,
  ResolveResult,
  WantedDependency,
} from '@pnpm/resolving.resolver-base'
import { type AquaResolveResult, resolveAqua } from '@pnpm/resolving.aqua-resolver'
import { resolveFromTarball, type TarballResolveResult } from '@pnpm/resolving.tarball-resolver'

export type {
  PackageMeta,
  PackageMetaCache,
  ResolveFunction,
  ResolverFactoryOptions,
}

export interface CustomResolverResolveResult extends ResolveResult {
  resolvedVia: 'custom-resolver'
}

export type DefaultResolveResult =
  | NpmResolveResult
  | JsrResolveResult
  | GitResolveResult
  | LocalResolveResult
  | TarballResolveResult
  | WorkspaceResolveResult
  | NodeRuntimeResolveResult
  | DenoRuntimeResolveResult
  | BunRuntimeResolveResult
  | AquaResolveResult
  | CustomResolverResolveResult

export type DefaultResolver = (wantedDependency: WantedDependency, opts: ResolveOptions) => Promise<DefaultResolveResult>

async function resolveFromCustomResolvers (
  customResolvers: CustomResolver[],
  wantedDependency: WantedDependency,
  opts: ResolveOptions
): Promise<DefaultResolveResult | null> {
  if (!customResolvers || customResolvers.length === 0) {
    return null
  }

  for (const customResolver of customResolvers) {
    // Skip custom resolvers that don't support both canResolve and resolve
    if (!customResolver.canResolve || !customResolver.resolve) continue

    // eslint-disable-next-line no-await-in-loop
    const canResolve = await checkCustomResolverCanResolve(customResolver, wantedDependency)

    if (canResolve) {
      // eslint-disable-next-line no-await-in-loop
      const result = await customResolver.resolve(wantedDependency, {
        lockfileDir: opts.lockfileDir,
        projectDir: opts.projectDir,
        preferredVersions: (opts.preferredVersions ?? {}) as unknown as Record<string, string>,
        currentPkg: opts.currentPkg,
      })
      return {
        ...result,
        resolvedVia: 'custom-resolver',
      } as DefaultResolveResult
    }
  }

  return null
}

export function createResolver (
  fetchFromRegistry: FetchFromRegistry,
  getAuthHeader: GetAuthHeader,
  pnpmOpts: ResolverFactoryOptions & {
    rawConfig: Record<string, string>
    customResolvers?: CustomResolver[]
  }
): { resolve: DefaultResolver, clearCache: () => void } {
  const { resolveFromNpm, resolveFromJsr, clearCache } = createNpmResolver(fetchFromRegistry, getAuthHeader, pnpmOpts)
  const resolveFromGit = createGitResolver(pnpmOpts)
  const _resolveFromLocal = resolveFromLocal.bind(null, {
    preserveAbsolutePaths: pnpmOpts.preserveAbsolutePaths,
  })
  const _resolveNodeRuntime = resolveNodeRuntime.bind(null, { fetchFromRegistry, offline: pnpmOpts.offline, rawConfig: pnpmOpts.rawConfig })
  const _resolveDenoRuntime = resolveDenoRuntime.bind(null, { fetchFromRegistry, offline: pnpmOpts.offline, rawConfig: pnpmOpts.rawConfig, resolveFromNpm })
  const _resolveBunRuntime = resolveBunRuntime.bind(null, { fetchFromRegistry, offline: pnpmOpts.offline, rawConfig: pnpmOpts.rawConfig, resolveFromNpm })
  const _resolveAqua = resolveAqua.bind(null, { fetchFromRegistry, offline: pnpmOpts.offline })
  const _resolveFromCustomResolvers = pnpmOpts.customResolvers
    ? resolveFromCustomResolvers.bind(null, pnpmOpts.customResolvers)
    : null
  return {
    resolve: async (wantedDependency, opts) => {
      const resolution = await _resolveFromCustomResolvers?.(wantedDependency, opts) ??
        await resolveFromNpm(wantedDependency, opts as ResolveFromNpmOptions) ??
        await resolveFromJsr(wantedDependency, opts as ResolveFromNpmOptions) ??
        (wantedDependency.bareSpecifier && (
          await resolveFromGit(wantedDependency as { bareSpecifier: string }, opts) ??
          await resolveFromTarball(fetchFromRegistry, wantedDependency as { bareSpecifier: string }) ??
          await _resolveAqua(wantedDependency, opts) ??
          await _resolveFromLocal(wantedDependency as { bareSpecifier: string }, opts)
        )) ??
        await _resolveNodeRuntime(wantedDependency, opts) ??
        await _resolveDenoRuntime(wantedDependency, opts) ??
        await _resolveBunRuntime(wantedDependency, opts)
      if (!resolution) {
        let specifier = `${wantedDependency.alias ? wantedDependency.alias + '@' : ''}${wantedDependency.bareSpecifier ?? ''}`
        if (specifier !== '') {
          specifier = `"${specifier}"`
        }
        throw new PnpmError(
          'SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER',
          `${specifier} isn't supported by any available resolver.`)
      }
      return resolution
    },
    clearCache,
  }
}
