import { PnpmError } from '@pnpm/error'
import { type FetchFromRegistry, type GetAuthHeader } from '@pnpm/fetching-types'
import { type GitResolveResult, createGitResolver } from '@pnpm/git-resolver'
import { type LocalResolveResult, resolveFromLocal } from '@pnpm/local-resolver'
import { resolveNodeRuntime, type NodeRuntimeResolveResult } from '@pnpm/node.resolver'
import { resolveDenoRuntime, type DenoRuntimeResolveResult } from '@pnpm/resolving.deno-resolver'
import { resolveBunRuntime, type BunRuntimeResolveResult } from '@pnpm/resolving.bun-resolver'
import {
  createNpmResolver,
  type JsrResolveResult,
  type NpmResolveResult,
  type WorkspaceResolveResult,
  type PackageMeta,
  type PackageMetaCache,
  type ResolveFromNpmOptions,
  type ResolverFactoryOptions,
} from '@pnpm/npm-resolver'
import {
  type ResolveFunction,
  type ResolveOptions,
  type ResolveResult,
  type WantedDependency,
} from '@pnpm/resolver-base'
import { type TarballResolveResult, resolveFromTarball } from '@pnpm/tarball-resolver'
import { type Adapter, checkAdapterCanResolve } from '@pnpm/hooks.types'

export type {
  PackageMeta,
  PackageMetaCache,
  ResolveFunction,
  ResolverFactoryOptions,
}

export interface AdapterResolveResult extends ResolveResult {
  resolvedVia: 'adapter'
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
  | AdapterResolveResult

export type DefaultResolver = (wantedDependency: WantedDependency, opts: ResolveOptions) => Promise<DefaultResolveResult>

async function resolveFromAdapters (
  adapters: Adapter[],
  wantedDependency: WantedDependency,
  opts: ResolveOptions
): Promise<DefaultResolveResult | null> {
  if (!adapters || adapters.length === 0) {
    return null
  }

  for (const adapter of adapters) {
    // Skip adapters that don't support both canResolve and resolve
    if (!adapter.canResolve || !adapter.resolve) continue

    // eslint-disable-next-line no-await-in-loop
    const canResolve = await checkAdapterCanResolve(adapter, wantedDependency)

    if (canResolve) {
      // eslint-disable-next-line no-await-in-loop
      const result = await adapter.resolve(wantedDependency, {
        lockfileDir: opts.lockfileDir,
        projectDir: opts.projectDir,
        preferredVersions: (opts.preferredVersions ?? {}) as unknown as Record<string, string>,
      })
      return {
        ...result,
        resolvedVia: 'adapter',
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
    adapters?: Adapter[]
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
  const _resolveFromAdapters = pnpmOpts.adapters
    ? resolveFromAdapters.bind(null, pnpmOpts.adapters)
    : null
  return {
    resolve: async (wantedDependency, opts) => {
      const resolution = (_resolveFromAdapters && await _resolveFromAdapters(wantedDependency, opts)) ??
        await resolveFromNpm(wantedDependency, opts as ResolveFromNpmOptions) ??
        await resolveFromJsr(wantedDependency, opts as ResolveFromNpmOptions) ??
        (wantedDependency.bareSpecifier && (
          await resolveFromTarball(fetchFromRegistry, wantedDependency as { bareSpecifier: string }) ??
          await resolveFromGit(wantedDependency as { bareSpecifier: string }) ??
          await _resolveFromLocal(wantedDependency as { bareSpecifier: string }, opts)
        )) ??
        await _resolveNodeRuntime(wantedDependency) ??
        await _resolveDenoRuntime(wantedDependency) ??
        await _resolveBunRuntime(wantedDependency)
      if (!resolution) {
        throw new PnpmError(
          'SPEC_NOT_SUPPORTED_BY_ANY_RESOLVER',
          `${wantedDependency.alias ? wantedDependency.alias + '@' : ''}${wantedDependency.bareSpecifier ?? ''} isn't supported by any available resolver.`)
      }
      return resolution
    },
    clearCache,
  }
}
