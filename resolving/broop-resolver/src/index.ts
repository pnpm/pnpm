import { PnpmError } from '@pnpm/error'
import type { FetchFromRegistry } from '@pnpm/fetching-types'
import type {
  PlatformAssetResolution,
  ResolveOptions,
  ResolveResult,
  VariationsResolution,
  WantedDependency,
} from '@pnpm/resolver-base'
import type { PkgResolutionId } from '@pnpm/types'
import type { BinaryResolution } from '@pnpm/resolver-base'
import { lexCompare } from '@pnpm/util.lex-comparator'
import { resolveFromHomebrew } from './homebrew.js'
import { resolveFromScoop } from './scoop.js'

export interface BroopResolveResult extends ResolveResult {
  resolution: VariationsResolution
  resolvedVia: 'broop'
}

export async function resolveBroop (
  ctx: {
    fetchFromRegistry: FetchFromRegistry
    offline?: boolean
  },
  wantedDependency: WantedDependency,
  opts?: Partial<ResolveOptions>
): Promise<BroopResolveResult | null> {
  const specifier = wantedDependency.bareSpecifier
  if (!specifier?.startsWith('broop:')) return null

  if (ctx.offline) {
    throw new PnpmError('BROOP_OFFLINE', 'Cannot resolve broop packages in offline mode')
  }

  if (opts?.currentPkg && !opts.update) {
    return {
      id: opts.currentPkg.id,
      resolution: opts.currentPkg.resolution as VariationsResolution,
      resolvedVia: 'broop',
    }
  }

  const { name, versionSpec } = parseBroopSpecifier(specifier)

  const [homebrewResult, scoopResult] = await Promise.allSettled([
    resolveFromHomebrew(ctx.fetchFromRegistry, name, versionSpec),
    resolveFromScoop(ctx.fetchFromRegistry, name, versionSpec),
  ])

  const assets: PlatformAssetResolution[] = []
  let version: string | undefined
  const allDeps = new Set<string>()

  if (homebrewResult.status === 'fulfilled') {
    assets.push(...homebrewResult.value.assets)
    version = homebrewResult.value.version
    for (const dep of homebrewResult.value.dependencies) {
      allDeps.add(dep)
    }
  }
  if (scoopResult.status === 'fulfilled') {
    assets.push(...scoopResult.value.assets)
    version ??= scoopResult.value.version
    for (const dep of scoopResult.value.dependencies) {
      allDeps.add(dep)
    }
  }

  if (assets.length === 0) {
    const errors = [
      homebrewResult.status === 'rejected' ? (homebrewResult.reason as Error).message : null,
      scoopResult.status === 'rejected' ? (scoopResult.reason as Error).message : null,
    ].filter(Boolean)
    throw new PnpmError(
      'BROOP_NOT_FOUND',
      `Could not find "${name}" in Homebrew or Scoop: ${errors.join('; ')}`
    )
  }

  assets.sort((a, b) => lexCompare(
    (a.resolution as BinaryResolution).url,
    (b.resolution as BinaryResolution).url
  ))

  return {
    id: `${name}@broop:${version}` as PkgResolutionId,
    normalizedBareSpecifier: `broop:${name}@${version}`,
    alias: wantedDependency.alias ?? name,
    resolvedVia: 'broop',
    manifest: {
      name,
      version: version!,
      // Each dependency is prefixed with broop: so pnpm's core engine
      // will recursively resolve them through this same resolver.
      // We use optionalDependencies because Homebrew and Scoop have
      // different dependency sets — a Scoop-only dep (e.g. cacert) won't
      // resolve on macOS, and vice versa. Optional deps are skipped
      // gracefully when no variant matches the current platform.
      ...(allDeps.size > 0 && {
        optionalDependencies: Object.fromEntries(
          [...allDeps].map((dep) => [dep, `broop:${dep}`])
        ),
      }),
    },
    resolution: {
      type: 'variations',
      variants: assets,
    },
  }
}

function parseBroopSpecifier (specifier: string): { name: string, versionSpec?: string } {
  // "broop:ripgrep" or "broop:ripgrep@15.1.0"
  const rest = specifier.substring('broop:'.length)
  const atIdx = rest.lastIndexOf('@')
  if (atIdx <= 0) {
    return { name: rest }
  }
  return {
    name: rest.substring(0, atIdx),
    versionSpec: rest.substring(atIdx + 1),
  }
}
