import type {
  NonDeprecatedAlternative,
  PkgResolutionId,
  ResolutionPolicyViolation,
  TarballResolution,
  WantedDependency,
} from '@pnpm/resolving.resolver-base'
import type { DependencyManifest } from '@pnpm/types'

import { calcPrefixedSpecifier } from './calcSpecifier.js'
import { NoMatchingVersionError } from './NoMatchingVersionError.js'
import { createRegistryTarballResolution, selectPackageRevision } from './packageRevision.js'
import {
  parseJsrSpecifierToRegistryPackageSpec,
  parseNamedRegistrySpecifierToRegistryPackageSpec,
} from './parseBareSpecifier.js'
import { findNonDeprecatedAlternative } from './pickPackageFromMeta.js'
import { createPickPackageOptions } from './pickPackageOptions.js'
import { warnOnceOnHeldBackUpdate } from './preferredVersionSelectors.js'
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

  const picked = await pickFromSimpleRegistry({ ctx, wantedDependency, opts, spec, registry: ctx.registriesByScope['@jsr']! }) // '@jsr' is always defined
  return {
    ...picked,
    normalizedBareSpecifier: opts.calcSpecifier
      ? calcPrefixedSpecifier({
        prefix: 'jsr:',
        pkgName: spec.jsrPkgName,
        wantedDependency,
        version: picked.manifest.version,
        revision: spec.revision,
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
    // Qualifying the id with the registry alias is what keeps the same
    // name@version resolved from two registries distinct in the lockfile.
    // Without it they collapse onto one entry and whichever resolved first
    // decides the tarball both consumers get.
    id: `${picked.manifest.name}@${spec.registryName}:${picked.manifest.version}` as PkgResolutionId,
    normalizedBareSpecifier: opts.calcSpecifier
      ? calcPrefixedSpecifier({
        prefix: `${spec.registryName}:`,
        pkgName: spec.name,
        wantedDependency,
        version: picked.manifest.version,
        revision: spec.revision,
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
// fields common to their result envelopes; each caller adds its own
// resolvedVia, alias, and normalizedBareSpecifier.
async function pickFromSimpleRegistry (request: RegistrySpecRequest): Promise<{
  id: PkgResolutionId
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
  const selectedPackage = selectPackageRevision(pickedPackage, spec, registry)
  const resolution = createRegistryTarballResolution(selectedPackage.dist, registry)
  const publishedAt = meta.time?.[pickedPackage.version]
  return {
    id: `${pickedPackage.name}@${pickedPackage.version}` as PkgResolutionId,
    latest: latestAllowedByPolicy(meta, opts),
    // Only worked out for a deprecated pick, so the scan stays on the rare path.
    nonDeprecatedAlternative: pickedPackage.deprecated
      ? findNonDeprecatedAlternative(meta, spec, opts)
      : undefined,
    manifest: selectedPackage,
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
