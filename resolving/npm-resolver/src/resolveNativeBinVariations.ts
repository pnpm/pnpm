import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import type { PackageInRegistry, PackageMeta } from '@pnpm/resolving.registry.types'
import type {
  PlatformAssetResolution,
  PlatformAssetTarget,
  VariationsResolution,
} from '@pnpm/resolving.resolver-base'
import type { Registries } from '@pnpm/types'
import versionSelectorType from 'version-selector-type'

import { normalizeRegistryUrl } from './normalizeRegistryUrl.js'
import type { RegistryPackageSpec } from './parseBareSpecifier.js'
import type { PickPackageOptions } from './pickPackage.js'

/**
 * Packages that ship a JS launcher shim plus per-platform native binaries as
 * `optionalDependencies` are always treated as native bin dependencies. Users
 * extend this set via the `nativeBinDependencies` setting.
 */
export const DEFAULT_NATIVE_BIN_DEPENDENCIES: readonly string[] = [
  'pacquet',
  '@pnpm/pacquet',
]

type PickPackage = (
  spec: RegistryPackageSpec,
  opts: PickPackageOptions
) => Promise<{ meta: PackageMeta, pickedPackage: PackageInRegistry | null }>

export interface ResolveNativeBinVariationsContext {
  pickPackage: PickPackage
  getAuthHeaderValueByURI: (uri: string, opts: { pkgName?: string }) => string | undefined
  registries: Registries
}

export interface NativeBinResolveResult {
  resolution: VariationsResolution
  bin: Record<string, string>
}

/**
 * Build a {@link VariationsResolution} for a wrapper package whose
 * `optionalDependencies` are per-platform native binary packages (e.g.
 * `pacquet` -> `@pacquet/darwin-arm64`, `@pacquet/linux-x64-musl`, ...). Each
 * optional dependency becomes one platform variant pointing directly at that
 * package's tarball, so only the host's variant is fetched and its binary is
 * linked with no launcher shim and no lifecycle scripts.
 *
 * Returns `null` when the wrapper has no platform-tagged optional dependency
 * (so the caller falls back to a normal tarball resolution). This mirrors the
 * guard Bun's native-binlink optimization uses.
 */
export async function resolveNativeBinVariations (
  ctx: ResolveNativeBinVariationsContext,
  wrapper: PackageInRegistry,
  opts: { dryRun?: boolean }
): Promise<NativeBinResolveResult | null> {
  const optionalDependencies = wrapper.optionalDependencies
  if (optionalDependencies == null) return null

  // The launcher shim's command names; the native binary inside each platform
  // package sits at the package root under the command name (`.exe` on Windows).
  // This matches the layout pacquet/@pnpm/exe publish and Bun's bin resolution.
  const commandNames = getCommandNames(wrapper)
  if (commandNames.length === 0) return null

  const variants = (await Promise.all(
    Object.entries(optionalDependencies).map(
      async ([depName, depBareSpecifier]): Promise<PlatformAssetResolution | null> => {
        const selector = versionSelectorType(depBareSpecifier)
        if (selector == null) return null
        const registry = pickRegistryForPackage(ctx.registries, depName, depBareSpecifier)
        const { pickedPackage } = await ctx.pickPackage(
          { type: selector.type, name: depName, fetchSpec: selector.normalized },
          {
            registry,
            dryRun: opts.dryRun === true,
            authHeaderValue: ctx.getAuthHeaderValueByURI(registry, { pkgName: depName }),
            preferredVersionSelectors: undefined,
          }
        )
        if (pickedPackage == null) return null
        const targets = getPlatformTargets(pickedPackage)
        if (targets.length === 0) return null
        const integrity = pickedPackage.dist.integrity
        if (integrity == null) return null
        const ext = targets.every((target) => target.os === 'win32') ? '.exe' : ''
        return {
          resolution: {
            type: 'binary',
            archive: 'tarball',
            url: normalizeRegistryUrl(pickedPackage.dist.tarball),
            integrity,
            bin: binPaths(commandNames, ext),
          },
          targets,
        }
      }
    )
  )).filter((variant): variant is PlatformAssetResolution => variant != null)

  if (variants.length === 0) return null
  return {
    resolution: { type: 'variations', variants },
    bin: binPaths(commandNames, process.platform === 'win32' ? '.exe' : ''),
  }
}

function getCommandNames (manifest: PackageInRegistry): string[] {
  // Wrapper packages declare their launcher under `bin`. The object form lists
  // the command names directly; the string form names a single command after
  // the (unscoped) package name.
  if (manifest.bin != null && typeof manifest.bin === 'object') {
    return Object.keys(manifest.bin)
  }
  if (typeof manifest.bin === 'string') {
    return [scopelessName(manifest.name)]
  }
  return []
}

function binPaths (commandNames: string[], ext: string): Record<string, string> {
  const bin: Record<string, string> = {}
  for (const name of commandNames) {
    bin[name] = `${name}${ext}`
  }
  return bin
}

function getPlatformTargets (manifest: PackageInRegistry): PlatformAssetTarget[] {
  // A platform package constrains itself with positive `os`/`cpu` (and `libc`
  // for musl) lists. Skip negations and entries missing os/cpu — those aren't
  // the per-platform packages this optimization targets.
  if (!manifest.os?.length || !manifest.cpu?.length) return []
  const libcValues = manifest.libc?.length ? manifest.libc : [undefined]
  const targets: PlatformAssetTarget[] = []
  for (const os of manifest.os) {
    if (os.startsWith('!')) continue
    for (const cpu of manifest.cpu) {
      if (cpu.startsWith('!')) continue
      for (const libc of libcValues) {
        targets.push({ os, cpu, ...(libc === 'musl' ? { libc: 'musl' } : {}) })
      }
    }
  }
  return targets
}

function scopelessName (name: string): string {
  const slashIndex = name.indexOf('/')
  return slashIndex === -1 ? name : name.slice(slashIndex + 1)
}
