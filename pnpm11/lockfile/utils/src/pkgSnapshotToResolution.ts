import url from 'node:url'

import { getRegistryServerType, normalizeRegistriesByPrefix } from '@pnpm/config.normalize-registries'
import * as dp from '@pnpm/deps.path'
import { PnpmError } from '@pnpm/error'
import type { PackageSnapshot, TarballResolution } from '@pnpm/lockfile.types'
import type { Resolution } from '@pnpm/resolving.resolver-base'
import { createRegistryRevisionsUnsupportedError, getNpmTarballUrl } from '@pnpm/resolving.tarball-url'
import type { RegistryContext } from '@pnpm/types'

import { nameVerFromPkgSnapshot } from './nameVerFromPkgSnapshot.js'

/** The registry facts alone; the tarball URL is rebuilt from them. */
export type PkgSnapshotToResolutionOptions = RegistryContext

export function pkgSnapshotToResolution (
  depPath: string,
  pkgSnapshot: PackageSnapshot,
  opts: PkgSnapshotToResolutionOptions
): Resolution {
  const resolution = pkgSnapshot.resolution as TarballResolution
  validateTarballResolution(depPath, resolution)
  if (isNonRegistryTarballResolution(resolution)) {
    return pkgSnapshot.resolution as Resolution
  }
  const nonSemverVersion = dp.parse(depPath).nonSemverVersion
  if (nonSemverVersion?.startsWith('file:')) {
    return {
      ...pkgSnapshot.resolution,
      tarball: nonSemverVersion,
    } as Resolution
  }
  const { name, version, registryName } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
  const registry = resolveRegistryForDep(depPath, registryName, name, opts)
  const tarball = resolveTarballUrl({
    depPath,
    name,
    opts,
    registry,
    resolution,
    version,
  })
  return {
    ...pkgSnapshot.resolution,
    tarball,
  } as Resolution
}

function validateTarballResolution (depPath: string, resolution: TarballResolution): void {
  if (resolution.tarball != null && typeof resolution.tarball !== 'string') {
    // Avoid URL string-coercion from malformed YAML lockfile values.
    throw new PnpmError('INVALID_TARBALL_RESOLUTION',
      `Cannot install package "${depPath}": its lockfile entry has a non-string "tarball" field.`)
  }
  if ((resolution as { revision?: unknown }).revision != null) {
    throw createRegistryRevisionsUnsupportedError(
      `Cannot install package "${depPath}": its lockfile entry pins a registry revision.`)
  }
}

function isNonRegistryTarballResolution (resolution: TarballResolution): boolean {
  return Boolean(resolution.type) ||
    Boolean(resolution.tarball?.startsWith('file:')) ||
    resolution.gitHosted === true
}

function resolveRegistryForDep (
  depPath: string,
  registryName: string | undefined,
  name: string | undefined,
  opts: PkgSnapshotToResolutionOptions
): string {
  if (registryName != null) {
    const registry = normalizeRegistriesByPrefix(opts.registriesByPrefix)[registryName]
    if (!registry) {
      throw new PnpmError('MISSING_NAMED_REGISTRY',
        `Cannot install package "${depPath}": its registry prefix '${registryName}:' is not declared by the registries setting.`,
        { hint: `Add a registries entry with "prefix: ${registryName}" to pnpm-workspace.yaml.` })
    }
    return registry
  }
  if (name != null && name[0] === '@') {
    const scopedRegistry = opts.registriesByScope[name.split('/')[0]]
    if (scopedRegistry) return scopedRegistry
  }
  return opts.registriesByScope.default
}

interface ResolveTarballUrlParams {
  depPath: string
  name: string | undefined
  opts: PkgSnapshotToResolutionOptions
  registry: string
  resolution: TarballResolution
  version: string | undefined
}

function resolveTarballUrl (params: ResolveTarballUrlParams): string {
  const { depPath, name, opts, registry, resolution, version } = params
  if (!resolution.tarball) {
    if (!name || !version) {
      throw new Error(`Couldn't get tarball URL from dependency path ${depPath}`)
    }
    return getNpmTarballUrl(name, version, {
      registry,
      serverType: getRegistryServerType(opts, registry),
    })
  }
  return new url.URL(resolution.tarball,
    registry.endsWith('/') ? registry : `${registry}/`
  ).toString()
}
