import { pickDeclaredRegistryPrefix } from '@pnpm/config.pick-registry-for-package'
import type {
  NonDeprecatedAlternative,
  ResolutionPolicyViolation,
  TarballResolution,
  WantedDependency,
} from '@pnpm/resolving.resolver-base'
import type { DependencyManifest } from '@pnpm/types'

import { calcPrefixedSpecifier } from './calcSpecifier.js'
import { NoMatchingVersionError } from './NoMatchingVersionError.js'
import {
  parseJsrSpecifierToRegistryPackageSpec,
  parseNamedRegistrySpecifierToRegistryPackageSpec,
} from './parseBareSpecifier.js'
import { findNonDeprecatedAlternative } from './pickPackageFromMeta.js'
import { createPickPackageOptions } from './pickPackageOptions.js'
import { warnOnceOnHeldBackUpdate } from './preferredVersionSelectors.js'
import { qualifiedPackageId, registryPackageId } from './registryPackageId.js'
import { createRegistryTarballResolution } from './registryTarballResolution.js'
import { detectMinReleaseAgeViolation, latestAllowedByPolicy } from './releaseAgePolicy.js'
import type {
  JsrResolveResult,
  NamedRegistryResolveResult,
  RegistrySpecRequest,
  ResolveFromNpmContext,
  ResolveFromNpmOptions,
} from './resolverTypes.js'

export async function resolveJsr (
  ctx: ResolveFromNpmContext,
  wantedDependency: WantedDependency & { optional?: boolean },
  opts: Omit<ResolveFromNpmOptions, 'registry'>
): Promise<JsrResolveResult | null> {
  if (!wantedDependency.bareSpecifier) return null

  const spec = parseJsrSpecifierToRegistryPackageSpec(wantedDependency.bareSpecifier, wantedDependency.alias, opts.defaultTag ?? 'latest')
  if (spec == null) return null

  const registry = ctx.registriesByScope['@jsr']! // '@jsr' is always defined
  const picked = await pickFromSimpleRegistry({ ctx, wantedDependency, opts, spec, registry })
  return {
    ...picked,
    id: registryPackageId(ctx, { name: picked.manifest.name, version: picked.manifest.version, registry }),
    normalizedBareSpecifier: opts.calcSpecifier
      ? calcPrefixedSpecifier({
        prefix: 'jsr:',
        pkgName: spec.jsrPkgName,
        wantedDependency,
        version: picked.manifest.version,
        defaultRangeSpecStyle: opts.rangeSpecStyle,
        isUpdate: Boolean(opts.updateRequested),
      })
      : undefined,
    resolvedVia: 'jsr-registry',
    alias: spec.jsrPkgName,
  }
}

// Resolves a `<alias>:` specifier from one of the configured named registries.
// The `gh:` alias ships as a built-in default pointing at the GitHub Packages
// npm registry; additional aliases come from pnpm-workspace.yaml's
// `registriesByPrefix` field. Auth tokens are looked up by the resolved registry
// URL, so a `//npm.pkg.github.com/:_authToken=...` entry in `.npmrc` is
// picked up automatically for `gh:` specifiers (and analogously for any user-
// configured alias).
export async function resolveFromNamedRegistry (
  ctx: ResolveFromNpmContext,
  wantedDependency: WantedDependency & { optional?: boolean },
  opts: Omit<ResolveFromNpmOptions, 'registry'>
): Promise<NamedRegistryResolveResult | null> {
  if (!wantedDependency.bareSpecifier) return null

  const spec = parseNamedRegistrySpecifierToRegistryPackageSpec(
    wantedDependency.bareSpecifier,
    ctx.namedRegistryNames,
    wantedDependency.alias,
    opts.defaultTag ?? 'latest'
  )
  if (spec == null) return null

  const registry = ctx.registriesByPrefix[spec.registryName]
  if (!registry) return null // defensive: should never trigger because parse checks the alias set

  const picked = await pickFromSimpleRegistry({ ctx, wantedDependency, opts, spec, registry })
  return {
    ...picked,
    // Qualifying the id with the registry is what keeps the same name@version
    // resolved from two registries distinct in the lockfile. Without it they
    // collapse onto one entry and whichever resolved first decides the
    // tarball both consumers get. A built-in alias that no declared prefix
    // shares a registry with qualifies the id with its own name.
    id: qualifiedPackageId(
      picked.manifest.name,
      picked.manifest.version,
      pickDeclaredRegistryPrefix(ctx.declaredRegistriesByPrefix, registry) ?? spec.registryName
    ),
    normalizedBareSpecifier: opts.calcSpecifier
      ? calcPrefixedSpecifier({
        prefix: `${spec.registryName}:`,
        pkgName: spec.name,
        wantedDependency,
        version: picked.manifest.version,
        defaultRangeSpecStyle: opts.rangeSpecStyle,
        isUpdate: Boolean(opts.updateRequested),
      })
      : undefined,
    resolvedVia: 'named-registry',
    registryName: spec.registryName,
    // Exposes the scoped package name so callers that omit an explicit alias
    // (e.g. `pnpm add gh:@acme/foo`) record the dependency under `@acme/foo`.
    alias: spec.name,
  }
}

// Shared inner shell for resolvers that pull from a single registry URL with
// an already-parsed RegistryPackageSpec (jsr, named-registry). Returns the
// fields common to their result envelopes; each caller adds its own id,
// resolvedVia, alias, and normalizedBareSpecifier.
async function pickFromSimpleRegistry (request: RegistrySpecRequest): Promise<{
  latest?: string
  nonDeprecatedAlternative?: NonDeprecatedAlternative
  manifest: DependencyManifest
  resolution: TarballResolution
  publishedAt?: string
  policyViolation?: ResolutionPolicyViolation
}> {
  const { ctx, wantedDependency, opts, spec, registry } = request
  const { meta, pickedPackage } = await ctx.pickPackage(spec, createPickPackageOptions(request))
  if (pickedPackage == null) {
    throw new NoMatchingVersionError({ wantedDependency, packageMeta: meta, registry })
  }
  warnOnceOnHeldBackUpdate(ctx, opts, spec, meta, pickedPackage.version)
  const resolution = createRegistryTarballResolution(pickedPackage)
  const publishedAt = meta.time?.[pickedPackage.version]
  return {
    latest: latestAllowedByPolicy(meta, opts),
    // Only worked out for a deprecated pick, so the scan stays on the rare path.
    nonDeprecatedAlternative: pickedPackage.deprecated
      ? findNonDeprecatedAlternative(meta, spec, opts)
      : undefined,
    manifest: pickedPackage,
    resolution,
    publishedAt,
    policyViolation: detectMinReleaseAgeViolation({
      name: pickedPackage.name,
      version: pickedPackage.version,
      publishedAt,
      resolution,
      publishedBy: opts.publishedBy,
      publishedByExclude: opts.publishedByExclude,
    }),
  }
}
