import { packageManager } from '@pnpm/cli.meta'
import { type Config, getPackageManagerBootstrapConfig } from '@pnpm/config.reader'
import { createPackageVersionPolicyOrThrow, getPublishedByPolicy } from '@pnpm/config.version-policy'
import { type ClientOptions, createResolver, type ResolutionPolicyViolation } from '@pnpm/installing.client'
import { type FullMetadataPolicyOptions, shouldFetchFullMetadata } from '@pnpm/store.connection-manager'

export type ResolvePnpmVersionOptions =
  & Pick<Config, 'cacheDir' | 'dir'>
  & Pick<ClientOptions, 'retry' | 'timeout'>
  & FullMetadataPolicyOptions
  & Partial<Pick<Config,
  | 'lockfileDir'
  | 'minimumReleaseAge'
  | 'minimumReleaseAgeExclude'
  | 'minimumReleaseAgeIgnoreMissingTime'
  | 'minimumReleaseAgeStrict'
  | 'packageManagerNetworkConfig'
  | 'packageManagerRegistries'
  | 'trustPolicyExclude'
  | 'trustPolicyIgnoreAfter'
  >>

export interface ResolvedPnpmVersion {
  version: string
  /**
   * Set when the resolver picked a version despite the `minimumReleaseAge`
   * or `trustPolicy` gate. The caller decides what to do about it: the
   * running version is knowingly left out of the maturity cutoff (see
   * below), so a violation here is about the version being moved *to*.
   */
  policyViolation?: ResolutionPolicyViolation
}

/**
 * Resolve `pnpm@<bareSpecifier>` to an exact version.
 *
 * Every request goes through the trusted package-manager bootstrap config —
 * the same channel version switching uses — so a repo-controlled `.npmrc` or
 * workspace manifest cannot redirect the lookup or attach its own
 * credentials to it.
 *
 * Returns `undefined` when the specifier resolves to nothing.
 */
export async function resolvePnpmVersion (
  opts: ResolvePnpmVersionOptions,
  bareSpecifier: string
): Promise<ResolvedPnpmVersion | undefined> {
  return prepareResolvePnpmVersion(opts)(bareSpecifier)
}

/** Looks a `pnpm@<bareSpecifier>` up. See {@link prepareResolvePnpmVersion}. */
export type PnpmVersionLookup = (bareSpecifier: string) => Promise<ResolvedPnpmVersion | undefined>

/**
 * The two-phase form of {@link resolvePnpmVersion}: settings are read and
 * validated here, and only the returned function talks to a registry.
 *
 * The split exists for callers that treat an unreachable registry as
 * survivable. They can guard the lookup alone, so a misconfigured
 * `trustPolicyExclude` — or any other bad setting — still surfaces as the
 * error it is instead of being mistaken for a failed lookup.
 */
export function prepareResolvePnpmVersion (opts: ResolvePnpmVersionOptions): PnpmVersionLookup {
  // `minimumReleaseAge` is not part of `shouldFetchFullMetadata` because the
  // resolver upgrades abbreviated metadata to full on demand for the
  // maturity check, so it isn't requested up front here.
  const fullMetadata = shouldFetchFullMetadata(opts)
  const { resolve } = createResolver({
    ...opts,
    ...getPackageManagerBootstrapConfig(opts),
    fullMetadata,
    filterMetadata: fullMetadata,
    ignoreMissingTimeField: opts.minimumReleaseAgeIgnoreMissingTime,
  })
  // The running version is already on this machine, so hiding it behind the
  // maturity cutoff protects nothing — it only makes a dist-tag that points
  // at it fall back to an older release, downgrading the user (pnpm/pnpm#13883).
  const { publishedBy, publishedByExclude } = getPublishedByPolicy({
    ...opts,
    minimumReleaseAgeExclude: [
      ...opts.minimumReleaseAgeExclude ?? [],
      `${packageManager.name}@${packageManager.version}`,
    ],
  })
  const trustPolicyExclude = opts.trustPolicyExclude
    ? createPackageVersionPolicyOrThrow(opts.trustPolicyExclude, 'trustPolicyExclude')
    : undefined
  return async (bareSpecifier) => {
    const resolution = await resolve({ alias: packageManager.name, bareSpecifier }, {
      lockfileDir: opts.lockfileDir ?? opts.dir,
      preferredVersions: {},
      projectDir: opts.dir,
      publishedBy,
      publishedByExclude,
      // Unlike `dlx` (whose real install re-resolves through the store
      // controller), this `resolve` is the only version selection the callers
      // make, so the trust policy has to be passed here for the no-downgrade
      // check to run.
      trustPolicy: opts.trustPolicy,
      trustPolicyExclude,
      trustPolicyIgnoreAfter: opts.trustPolicyIgnoreAfter,
    })
    if (!resolution?.manifest) return undefined
    return {
      version: resolution.manifest.version,
      policyViolation: resolution.policyViolation,
    }
  }
}
