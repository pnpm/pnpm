import { type BunRuntimeResolveResult, resolveBunRuntime, resolveLatestBunRuntime } from '@pnpm/engine.runtime.bun-resolver'
import { type DenoRuntimeResolveResult, resolveDenoRuntime, resolveLatestDenoRuntime } from '@pnpm/engine.runtime.deno-resolver'
import { type NodeRuntimeResolveResult, resolveLatestNodeRuntime, resolveNodeRuntime } from '@pnpm/engine.runtime.node-resolver'
import { PnpmError } from '@pnpm/error'
import type { FetchFromRegistry, GetAuthHeader } from '@pnpm/fetching.types'
import { checkCustomResolverCanResolve, type CustomResolver } from '@pnpm/hooks.types'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createGitResolver, type GitResolveResult, resolveLatestFromGit } from '@pnpm/resolving.git-resolver'
import { type LocalResolveResult, resolveFromLocalPath, resolveFromLocalScheme, resolveLatestFromLocal } from '@pnpm/resolving.local-resolver'
import {
  createDefaultPackageMetaCache,
  createNpmResolutionVerifier,
  type CreateNpmResolutionVerifierOptions,
  createNpmResolver,
  type JsrResolveResult,
  type NamedRegistryResolveResult,
  type NpmResolver,
  type NpmResolveResult,
  type PackageMeta,
  type PackageMetaCache,
  type ResolveFromNpmOptions,
  type ResolverFactoryOptions,
  type WorkspaceResolveResult,
} from '@pnpm/resolving.npm-resolver'
import type {
  LatestInfo,
  LatestQuery,
  ResolutionVerifier,
  ResolveFunction,
  ResolveOptions,
  ResolveResult,
  WantedDependency,
} from '@pnpm/resolving.resolver-base'
import { resolveFromTarball, resolveLatestFromTarball, type TarballResolveResult } from '@pnpm/resolving.tarball-resolver'
import type { RegistryConfig } from '@pnpm/types'

export {
  createDefaultPackageMetaCache,
}

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
  | NamedRegistryResolveResult
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

    // eslint-disable-next-line no-await-in-loop -- the first custom resolver that accepts the dependency wins, so they are tried in order
    const canResolve = await checkCustomResolverCanResolve(customResolver, wantedDependency)

    if (canResolve) {
      // eslint-disable-next-line no-await-in-loop -- the first custom resolver that accepts the dependency wins, so they are tried in order
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

export type ResolveLatestDispatcher = (query: LatestQuery, opts: ResolveOptions) => Promise<LatestInfo | undefined>

export function createResolver (
  fetchFromRegistry: FetchFromRegistry,
  getAuthHeader: GetAuthHeader,
  pnpmOpts: ResolverFactoryOptions & {
    nodeDownloadMirrors?: Record<string, string>
    customResolvers?: CustomResolver[]
  }
): { resolve: DefaultResolver, resolveLatest: ResolveLatestDispatcher, clearCache: () => void } {
  const npmResolvers = createNpmResolver(fetchFromRegistry, getAuthHeader, pnpmOpts)
  const resolveFromGit = createGitResolver(pnpmOpts)
  const localCtx = { preserveAbsolutePaths: pnpmOpts.preserveAbsolutePaths }
  const runtimes = buildRuntimeResolvers(fetchFromRegistry, getAuthHeader, pnpmOpts, npmResolvers.resolveFromNpm)
  const customResolvers = pnpmOpts.customResolvers ? resolveFromCustomResolvers.bind(null, pnpmOpts.customResolvers) : null

  return {
    resolve: buildResolveFunction({
      customResolvers,
      npmResolvers,
      resolveFromGit,
      localCtx,
      runtimes,
      fetchFromRegistry,
    }),
    resolveLatest: buildResolveLatestFunction({
      npmResolvers,
      runtimes,
    }),
    clearCache: npmResolvers.clearCache,
  }
}

interface RuntimeResolvers {
  resolveNode: (wanted: WantedDependency, opts: ResolveOptions) => Promise<NodeRuntimeResolveResult | null>
  resolveDeno: (wanted: WantedDependency, opts: ResolveOptions) => Promise<DenoRuntimeResolveResult | null>
  resolveBun: (wanted: WantedDependency, opts: ResolveOptions) => Promise<BunRuntimeResolveResult | null>
  resolveLatestNode: (query: LatestQuery, opts: ResolveOptions) => Promise<LatestInfo | undefined>
  resolveLatestDeno: (query: LatestQuery, opts: ResolveOptions) => Promise<LatestInfo | undefined>
  resolveLatestBun: (query: LatestQuery, opts: ResolveOptions) => Promise<LatestInfo | undefined>
}

function buildRuntimeResolvers (
  fetchFromRegistry: FetchFromRegistry,
  getAuthHeader: GetAuthHeader,
  pnpmOpts: ResolverFactoryOptions & { nodeDownloadMirrors?: Record<string, string> },
  resolveFromNpm: NpmResolver
): RuntimeResolvers {
  return {
    resolveNode: resolveNodeRuntime.bind(null, { fetchFromRegistry, getAuthHeader, offline: pnpmOpts.offline, nodeDownloadMirrors: pnpmOpts.nodeDownloadMirrors, cacheDir: pnpmOpts.cacheDir }),
    resolveDeno: resolveDenoRuntime.bind(null, { fetchFromRegistry, offline: pnpmOpts.offline, resolveFromNpm }),
    resolveBun: resolveBunRuntime.bind(null, { fetchFromRegistry, offline: pnpmOpts.offline, resolveFromNpm }),
    resolveLatestNode: resolveLatestNodeRuntime.bind(null, { fetchFromRegistry, getAuthHeader, nodeDownloadMirrors: pnpmOpts.nodeDownloadMirrors }),
    resolveLatestDeno: resolveLatestDenoRuntime.bind(null, { resolveFromNpm }),
    resolveLatestBun: resolveLatestBunRuntime.bind(null, { resolveFromNpm }),
  }
}

interface ResolveFunctionContext {
  customResolvers: ((wantedDependency: WantedDependency, opts: ResolveOptions) => Promise<DefaultResolveResult | null>) | null
  npmResolvers: ReturnType<typeof createNpmResolver>
  resolveFromGit: ReturnType<typeof createGitResolver>
  localCtx: { preserveAbsolutePaths?: boolean }
  runtimes: RuntimeResolvers
  fetchFromRegistry: FetchFromRegistry
}

function buildResolveFunction (ctx: ResolveFunctionContext): DefaultResolver {
  return async (wantedDependency, opts) => {
    const resolution = await dispatchResolve(ctx, wantedDependency, opts)
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
  }
}

async function dispatchResolve (
  ctx: ResolveFunctionContext,
  wantedDependency: WantedDependency,
  opts: ResolveOptions
): Promise<DefaultResolveResult | null> {
  const spec = wantedDependency.bareSpecifier ? (wantedDependency as { bareSpecifier: string }) : null
  const resolution = await ctx.customResolvers?.(wantedDependency, opts) ??
    await ctx.npmResolvers.resolveFromNpm(wantedDependency, opts as ResolveFromNpmOptions) ??
    await ctx.npmResolvers.resolveFromJsr(wantedDependency, opts as ResolveFromNpmOptions) ??
    (spec && (
      await ctx.resolveFromGit(spec, opts) ??
      await resolveFromTarball(ctx.fetchFromRegistry, spec) ??
      await resolveFromLocalScheme(ctx.localCtx, spec, opts)
    )) ??
    await ctx.runtimes.resolveNode(wantedDependency, opts) ??
    await ctx.runtimes.resolveDeno(wantedDependency, opts) ??
    await ctx.runtimes.resolveBun(wantedDependency, opts) ??
    await ctx.npmResolvers.resolveFromNamedRegistry(wantedDependency, opts as ResolveFromNpmOptions) ??
    (spec ? await resolveFromLocalPath(ctx.localCtx, spec, opts) : null)
  return resolution
}

function buildResolveLatestFunction (ctx: {
  npmResolvers: ReturnType<typeof createNpmResolver>
  runtimes: RuntimeResolvers
}): ResolveLatestDispatcher {
  return async (query, opts) => {
    const info = (await ctx.npmResolvers.resolveLatestFromNpm(query, opts)) ??
      (await ctx.npmResolvers.resolveLatestFromJsr(query, opts)) ??
      (await resolveLatestFromGit(query)) ??
      (await resolveLatestFromTarball(query)) ??
      (await resolveLatestFromLocal(query)) ??
      (await ctx.runtimes.resolveLatestNode(query, opts)) ??
      (await ctx.runtimes.resolveLatestDeno(query, opts)) ??
      (await ctx.runtimes.resolveLatestBun(query, opts)) ??
      (await ctx.npmResolvers.resolveLatestFromNamedRegistry(query, opts))
    return info
  }
}


export type ResolutionVerifierFactoryOptions =
  & Pick<ResolverFactoryOptions, 'cacheDir' | 'registriesByScope' | 'registriesByPrefix' | 'offline' | 'retry' | 'timeout' | 'fetchWarnTimeoutMs'>
  & Pick<CreateNpmResolutionVerifierOptions,
  | 'minimumReleaseAge'
  | 'minimumReleaseAgeStrict'
  | 'minimumReleaseAgeExclude'
  | 'ignoreMissingTimeField'
  | 'trustPolicy'
  | 'trustPolicyExclude'
  | 'trustPolicyIgnoreAfter'
  | 'metaCache'
  | 'now'
  > & {
    configByUri?: Record<string, RegistryConfig>
  }

/**
 * Companion to {@link createResolver}. Collects the resolver-specific
 * verifier factories (today: npm) into a list. The npm verifier is
 * always present — it enforces the tarball-URL binding regardless of
 * policy configuration — so the list is non-empty.
 *
 * Future protocols (jsr, git, attestation, etc.) plug in here by pushing
 * their own `ResolutionVerifier` onto the list. Each verifier handles
 * its own protocol short-circuit inside `verify` (returns `{ ok: true }`
 * for resolutions outside its scope), so dispatch happens naturally at
 * the install side — no combinator needed.
 */
export function createResolutionVerifiers (
  fetchFromRegistry: FetchFromRegistry,
  opts: ResolutionVerifierFactoryOptions
): ResolutionVerifier[] {
  const fetchOpts = {
    fetch: fetchFromRegistry,
    retry: opts.retry ?? {},
    timeout: opts.timeout ?? 60_000,
    fetchWarnTimeoutMs: opts.fetchWarnTimeoutMs ?? 10_000,
  }
  const getAuthHeaderValueByURI = createGetAuthHeaderByURI(opts.configByUri ?? {})
  const verifiers: ResolutionVerifier[] = []
  const npmVerifier = createNpmResolutionVerifier({
    minimumReleaseAge: opts.minimumReleaseAge,
    minimumReleaseAgeStrict: opts.minimumReleaseAgeStrict,
    minimumReleaseAgeExclude: opts.minimumReleaseAgeExclude,
    ignoreMissingTimeField: opts.ignoreMissingTimeField,
    trustPolicy: opts.trustPolicy,
    trustPolicyExclude: opts.trustPolicyExclude,
    trustPolicyIgnoreAfter: opts.trustPolicyIgnoreAfter,
    registriesByScope: opts.registriesByScope,
    registriesByPrefix: opts.registriesByPrefix,
    fetchOpts,
    getAuthHeaderValueByURI,
    cacheDir: opts.cacheDir,
    offline: opts.offline,
    metaCache: opts.metaCache,
    now: opts.now,
  })
  verifiers.push(npmVerifier)
  return verifiers
}
