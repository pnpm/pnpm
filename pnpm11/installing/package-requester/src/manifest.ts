import { installabilityUnderForce, packageIsInstallable } from '@pnpm/config.package-is-installable'
import { fileSpecToPackageRootLink } from '@pnpm/resolving.local-resolver'
import type { RequestPackageOptions } from '@pnpm/store.controller-types'
import {
  DEPENDENCIES_OR_PEER_FIELDS,
  type DependencyManifest,
} from '@pnpm/types'

export function manifestForEngineCheck (
  manifest: DependencyManifest,
  { engineStrict, options }: { engineStrict: boolean, options: RequestPackageOptions }
): DependencyManifest {
  if (!engineStrict || options.deferEnginesCheck?.(manifest) !== true) return manifest
  return { ...manifest, engines: undefined }
}

/**
 * Rewrites the relative `file:` dependencies a package from a tarball or the
 * registry declares to point inside itself as `link:<root>/...` references.
 * It runs on the published manifest, before any `readPackage` hook or
 * override, so a `file:` specifier a hook writes keeps its usual meaning.
 */
export function linkFileDepsInsidePackage (manifest: DependencyManifest): DependencyManifest {
  let copy: DependencyManifest | undefined
  for (const depsField of ['dependencies', 'optionalDependencies'] as const) {
    for (const [alias, bareSpecifier] of Object.entries(manifest[depsField] ?? {})) {
      // A published manifest is unvalidated here, and a hook may still repair it.
      if (typeof bareSpecifier !== 'string') continue
      const packageRootLink = fileSpecToPackageRootLink(bareSpecifier)
      if (packageRootLink == null) continue
      copy ??= copyManifest(manifest)
      copy[depsField]![alias] = packageRootLink
    }
  }
  return copy ?? manifest
}

export function copyManifest (manifest: DependencyManifest): DependencyManifest {
  const copy: DependencyManifest = { ...manifest }
  for (const depsField of DEPENDENCIES_OR_PEER_FIELDS) {
    if (manifest[depsField] != null) {
      copy[depsField] = { ...manifest[depsField] }
    }
  }
  if (manifest.peerDependenciesMeta != null) {
    copy.peerDependenciesMeta = {}
    for (const [peerName, peerMeta] of Object.entries(manifest.peerDependenciesMeta)) {
      copy.peerDependenciesMeta[peerName] = { ...peerMeta }
    }
  }
  if (manifest.engines != null) {
    copy.engines = { ...manifest.engines }
  }
  return copy
}

export interface InstallabilityCheck {
  engineStrict: boolean
  id: string
  includeIncompatiblePackages: boolean
  nodeVersion?: string
  optional: boolean
  options: RequestPackageOptions
}

export async function applyReadPackageHook (
  readPackageHook: NonNullable<RequestPackageOptions['readPackageHook']>,
  manifest: DependencyManifest
): Promise<DependencyManifest> {
  const hookedManifest = await readPackageHook(copyManifest(manifest))
  return hookedManifest != null ? hookedManifest as DependencyManifest : manifest
}

export function createInstallabilityCheck (
  { ctx, id, options, wantedDependency }: {
    ctx: Parameters<typeof installabilityUnderForce>[0] & { nodeVersion?: string }
    id: string
    options: RequestPackageOptions
    wantedDependency: { optional?: boolean }
  }
): InstallabilityCheck {
  const { engineStrict, includeIncompatiblePackages } = installabilityUnderForce(ctx)
  return {
    engineStrict,
    id,
    includeIncompatiblePackages,
    nodeVersion: options.nodeVersion ?? ctx.nodeVersion,
    optional: wantedDependency.optional === true,
    options,
  }
}

export function checkInstallability (check: InstallabilityCheck, manifest: DependencyManifest): boolean | null {
  return packageIsInstallable(check.id, manifestForEngineCheck(manifest, check), {
    engineStrict: check.engineStrict,
    lockfileDir: check.options.lockfileDir,
    nodeVersion: check.nodeVersion,
    optional: check.optional,
    supportedArchitectures: check.options.supportedArchitectures,
  })
}
