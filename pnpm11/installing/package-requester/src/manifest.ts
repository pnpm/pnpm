import { installabilityUnderForce, packageIsInstallable } from '@pnpm/config.package-is-installable'
import { fileSpecToPackageRootLink } from '@pnpm/resolving.local-resolver'
import type { Resolution } from '@pnpm/resolving.resolver-base'
import type { PkgRequestFetchResult, RequestPackageOptions } from '@pnpm/store.controller-types'
import {
  DEPENDENCIES_OR_PEER_FIELDS,
  type DependencyManifest,
} from '@pnpm/types'
import { loadJsonFile } from 'load-json-file'

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

export async function prepareResolvedManifest (
  resolvedManifest: DependencyManifest | undefined,
  resolution: Resolution,
  options: RequestPackageOptions
): Promise<{ manifest?: DependencyManifest, unhookedManifest?: DependencyManifest, hooked: boolean }> {
  let unhookedManifest = resolvedManifest
  if (unhookedManifest != null && resolution.type !== 'directory') {
    unhookedManifest = linkFileDepsInsidePackage(unhookedManifest)
  }
  if (options.readPackageHook == null || unhookedManifest == null) {
    return { manifest: unhookedManifest, unhookedManifest, hooked: false }
  }
  return {
    manifest: await applyReadPackageHook(options.readPackageHook, unhookedManifest),
    unhookedManifest,
    hooked: true,
  }
}

export async function readFetchedManifest (fetchedResult: PkgRequestFetchResult): Promise<DependencyManifest | undefined> {
  let manifest: DependencyManifest | undefined
  if (fetchedResult.bundledManifest) {
    manifest = fetchedResult.bundledManifest as DependencyManifest
  } else if (fetchedResult.files.filesMap.has('package.json')) {
    const loadedManifest = await loadJsonFile<Record<string, unknown>>(fetchedResult.files.filesMap.get('package.json')!)
    // Skip placeholder package.json added as a completion marker by the worker
    // for packages that genuinely lack one.
    if (!loadedManifest._pnpmPlaceholder) {
      manifest = loadedManifest as unknown as DependencyManifest
    }
  }
  return manifest != null ? linkFileDepsInsidePackage(manifest) : manifest
}
