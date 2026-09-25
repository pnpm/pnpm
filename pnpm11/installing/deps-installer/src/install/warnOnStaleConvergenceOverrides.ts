import type { VersionOverride } from '@pnpm/config.parse-overrides'
import { getPublishedByPolicy } from '@pnpm/config.version-policy'
import { globalWarn } from '@pnpm/logger'
import type { RequestPackageFunction } from '@pnpm/store.controller-types'
import semver from 'semver'

export interface WarnOnStaleConvergenceOverridesOptions {
  convergeDeclaredRanges: Map<string, Set<string>>
  parsedOverrides: VersionOverride[]
  requestPackage: RequestPackageFunction
  lockfileDir: string
  minimumReleaseAge?: number
  minimumReleaseAgeExclude?: string[]
}

/**
 * A convergence override's value is derived state — the best version that
 * converges the currently declared ranges. It goes stale when dependents
 * start declaring ranges that admit newer versions: the override then keeps
 * the edges it governs on the old version while newer edges resolve past it,
 * producing the duplication the entry was written to prevent.
 *
 * Must only run after a full resolution: only then has every manifest
 * streamed through the versions overrider, so `convergeDeclaredRanges` holds
 * the complete set of declared ranges. For each convergence override this
 * resolves every declared range to the best version it admits (metadata is
 * already cached from the resolution that just ran, and the release-age
 * policy applies, so the recommendation is a version the resolver would
 * actually pick) and warns when a version newer than the override's value
 * satisfies every declared range — a strictly better convergence.
 *
 * A range that fails to resolve contributes no candidate but still
 * participates in the satisfies-every-range check, so failures can only
 * suppress the warning, never fabricate one.
 */
export async function warnOnStaleConvergenceOverrides (opts: WarnOnStaleConvergenceOverridesOptions): Promise<void> {
  const convergeOverrides = opts.parsedOverrides.filter(({ converge }) => converge)
  if (convergeOverrides.length === 0) return
  const { publishedBy, publishedByExclude } = getPublishedByPolicy(opts)
  await Promise.all(convergeOverrides.map(async (override) => {
    const name = override.targetPkg.name
    const ranges = opts.convergeDeclaredRanges.get(name)
    if (ranges == null || ranges.size === 0) return
    const candidates = await Promise.all([...ranges].map(async (range) => {
      try {
        const response = await opts.requestPackage({ alias: name, bareSpecifier: range }, {
          downloadPriority: 0,
          lockfileDir: opts.lockfileDir,
          projectDir: opts.lockfileDir,
          preferredVersions: {},
          skipFetch: true,
          publishedBy,
          publishedByExclude,
        })
        if (response.body.policyViolation != null) return undefined
        return response.body.manifest?.version
      } catch {
        return undefined
      }
    }))
    const best = candidates
      .filter((version): version is string => version != null && semver.gt(version, override.newBareSpecifier))
      .sort(semver.rcompare)
      .find((version) => [...ranges].every((range) => semver.satisfies(version, range, true)))
    if (best == null) return
    globalWarn(`The convergence override "${name}@": "${override.newBareSpecifier}" is stale: every declared range of ${name} also admits ${best}. Change the override's value to ${best} in pnpm-workspace.yaml, or remove the override and run "pnpm dedupe".`)
  }))
}
