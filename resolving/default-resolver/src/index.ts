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
import { type CustomResolver, checkCustomResolverCanResolve } from '@pnpm/hooks.types'

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
  const _resolveFromCustomResolvers = pnpmOpts.customResolvers
    ? resolveFromCustomResolvers.bind(null, pnpmOpts.customResolvers)
    : null
  return {
    resolve: async (wantedDependency, opts) => {
      const resolution = await _resolveFromCustomResolvers?.(wantedDependency, opts) ??
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
        let specifier = `${wantedDependency.alias ? wantedDependency.alias + '@' : ''}${wantedDependency.bareSpecifier ?? ''}`
        if (specifier) {
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
