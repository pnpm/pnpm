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
  type WantedDependency,
} from '@pnpm/resolver-base'
import { type TarballResolveResult, resolveFromTarball } from '@pnpm/tarball-resolver'
import { type ResolverPlugin, type PackageDescriptor } from '@pnpm/hooks.types'

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
  | DenoRuntimeResolveResult
  | BunRuntimeResolveResult

export type DefaultResolver = (wantedDependency: WantedDependency, opts: ResolveOptions) => Promise<DefaultResolveResult>

export function createResolver (
  fetchFromRegistry: FetchFromRegistry,
  getAuthHeader: GetAuthHeader,
  pnpmOpts: ResolverFactoryOptions & {
    rawConfig: Record<string, string>
    customResolvers?: ResolverPlugin[]
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
  return {
    resolve: async (wantedDependency, opts) => {
      // Try custom resolvers first
      if (pnpmOpts.customResolvers && pnpmOpts.customResolvers.length > 0) {
        const descriptor: PackageDescriptor = {
          name: wantedDependency.alias ?? '',
          range: wantedDependency.bareSpecifier ?? '',
        }

        for (const resolver of pnpmOpts.customResolvers) {
          // Skip resolvers that don't support descriptor resolution
          if (!resolver.supportsDescriptor || !resolver.resolve) continue

          const supportsResult = resolver.supportsDescriptor(descriptor)
          // eslint-disable-next-line no-await-in-loop
          const supports = supportsResult instanceof Promise ? await supportsResult : supportsResult
          if (supports) {
            const resolveResult = resolver.resolve(descriptor, {
              lockfileDir: opts.lockfileDir,
              projectDir: opts.projectDir,
              preferredVersions: (opts.preferredVersions ?? {}) as unknown as Record<string, string>,
            })
            // eslint-disable-next-line no-await-in-loop
            const result = resolveResult instanceof Promise ? await resolveResult : resolveResult
            return result as DefaultResolveResult
          }
        }
      }

      const resolution = await resolveFromNpm(wantedDependency, opts as ResolveFromNpmOptions) ??
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
