import { PnpmError } from '@pnpm/error'
import { type FetchFromRegistry, type GetAuthHeader } from '@pnpm/fetching-types'
import { type GitResolveResult, createGitResolver } from '@pnpm/git-resolver'
import { type LocalResolveResult, resolveFromLocal } from '@pnpm/local-resolver'
import { resolveNodeRuntime, type NodeRuntimeResolveResult } from '@pnpm/node.resolver'
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
  type WantedDependency,
} from '@pnpm/resolver-base'
import { type TarballResolveResult, resolveFromTarball } from '@pnpm/tarball-resolver'

export type {
  PackageMeta,
  PackageMetaCache,
  ResolveFunction,
  ResolverFactoryOptions,
}

export type DefaultResolveResult =
  | NpmResolveResult
  | JsrResolveResult
  | GitResolveResult
  | LocalResolveResult
  | TarballResolveResult
  | WorkspaceResolveResult
  | NodeRuntimeResolveResult

export type DefaultResolver = (wantedDependency: WantedDependency, opts: ResolveOptions) => Promise<DefaultResolveResult>

export function createResolver (
  fetchFromRegistry: FetchFromRegistry,
  getAuthHeader: GetAuthHeader,
  pnpmOpts: ResolverFactoryOptions & {
    rawConfig: Record<string, string>
  }
): { resolve: DefaultResolver, clearCache: () => void } {
  const { resolveFromNpm, resolveFromJsr, clearCache } = createNpmResolver(fetchFromRegistry, getAuthHeader, pnpmOpts)
  const resolveFromGit = createGitResolver(pnpmOpts)
  const _resolveNodeRuntime = resolveNodeRuntime.bind(null, { fetchFromRegistry, offline: pnpmOpts.offline, rawConfig: pnpmOpts.rawConfig })
  return {
    resolve: async (wantedDependency, opts) => {
      const resolution = await resolveFromNpm(wantedDependency, opts as ResolveFromNpmOptions) ??
        await resolveFromJsr(wantedDependency, opts as ResolveFromNpmOptions) ??
        (wantedDependency.bareSpecifier && (
          await resolveFromTarball(fetchFromRegistry, wantedDependency as { bareSpecifier: string }) ??
          await resolveFromGit(wantedDependency as { bareSpecifier: string }) ??
          await resolveFromLocal(wantedDependency as { bareSpecifier: string }, opts)
        )) ??
        await _resolveNodeRuntime(wantedDependency)
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
