import { PnpmError, redactUrlForDisplay } from '@pnpm/error'
import type { PackageInRegistry, PackageRevision } from '@pnpm/resolving.registry.types'
import type { TarballResolution } from '@pnpm/resolving.resolver-base'
import {
  isIntegrityAddressedRegistryTarballUrl,
  isValidTarballRevision,
} from '@pnpm/resolving.tarball-url'

import { getIntegrity } from './getIntegrity.js'
import { normalizeRegistryUrl } from './normalizeRegistryUrl.js'
import type { RegistryPackageSpec } from './parseBareSpecifier.js'

export function createRegistryTarballResolution (
  dist: PackageInRegistry['dist'],
  registry: string
): TarballResolution {
  const integrity = getIntegrity(dist)
  const tarball = normalizeRegistryUrl(dist.tarball)
  if (dist.revision == null) {
    return { integrity, tarball }
  }
  if (!isValidTarballRevision(dist.revision)) {
    throw new PnpmError('MALFORMED_METADATA',
      `Tarball "${redactUrlForDisplay(dist.tarball)}" has an invalid revision in its metadata: ${String(dist.revision)}`)
  }
  if (
    integrity == null ||
    !isIntegrityAddressedRegistryTarballUrl(tarball, integrity, registry)
  ) {
    throw new PnpmError('MALFORMED_METADATA',
      `Tarball "${redactUrlForDisplay(dist.tarball)}" has revision ${dist.revision} but is not addressed by its complete integrity.`)
  }
  return {
    integrity,
    revision: dist.revision,
    tarball,
  }
}

const REVISION_MANIFEST_FIELDS = [
  'bin',
  'bundleDependencies',
  'bundledDependencies',
  'cpu',
  'dependencies',
  'engines',
  'hasInstallScript',
  'libc',
  'optionalDependencies',
  'os',
  'peerDependencies',
  'peerDependenciesMeta',
] as const

export function selectPackageRevision (
  pickedPackage: PackageInRegistry,
  spec: RegistryPackageSpec,
  registry: string
): PackageInRegistry {
  validateCurrentPackageRevision(pickedPackage, registry)
  if (spec.revision == null) return pickedPackage
  if (pickedPackage.dist.revisions == null && spec.revision === 0 && pickedPackage.dist.revision == null) return pickedPackage
  const selectedRevision = findAdvertisedRevision(pickedPackage, spec.revision)
  validatePackageRevision(pickedPackage, selectedRevision, registry)
  return applyPackageRevision(pickedPackage, selectedRevision, spec.revision)
}

function findAdvertisedRevision (pickedPackage: PackageInRegistry, wantedRevision: number): PackageRevision {
  const revisions = pickedPackage.dist.revisions
  if (revisions == null) {
    throw noMatchingRevision(pickedPackage, wantedRevision)
  }
  if (!Array.isArray(revisions)) {
    throw malformedRevisionHistory(pickedPackage, 'the revisions field is not an array')
  }
  const matches = revisions.filter((entry) => isRevisionNumber(entry?.revision) && entry.revision === wantedRevision)
  if (matches.length === 0) {
    throw noMatchingRevision(pickedPackage, wantedRevision)
  }
  if (matches.length !== 1) {
    throw malformedRevisionHistory(pickedPackage, `revision ${wantedRevision} is advertised more than once`)
  }
  return matches[0]
}

function applyPackageRevision (
  pickedPackage: PackageInRegistry,
  selectedRevision: PackageRevision,
  wantedRevision: number
): PackageInRegistry {
  const selectedPackage = { ...pickedPackage } as PackageInRegistry
  for (const field of REVISION_MANIFEST_FIELDS) {
    delete selectedPackage[field]
    const value = selectedRevision.manifest[field]
    if (value !== undefined) {
      selectedPackage[field] = value as never
    }
  }
  selectedPackage.dist = {
    ...pickedPackage.dist,
    integrity: selectedRevision.integrity,
    tarball: selectedRevision.tarball,
  }
  if (wantedRevision === 0) {
    delete selectedPackage.dist.revision
  } else {
    selectedPackage.dist.revision = wantedRevision
  }
  return selectedPackage
}

function validateCurrentPackageRevision (
  pickedPackage: PackageInRegistry,
  registry: string
): void {
  const revision = pickedPackage.dist.revision
  if (revision == null) return
  if (!isValidTarballRevision(revision)) {
    throw malformedRevisionHistory(pickedPackage, `current revision ${String(revision)} is not a canonical positive safe integer`)
  }
  const revisions = pickedPackage.dist.revisions
  if (!Array.isArray(revisions)) {
    throw malformedRevisionHistory(pickedPackage, 'the current revision has no revision history')
  }
  const matches = revisions.filter(entry => entry?.revision === revision)
  if (matches.length !== 1) {
    throw malformedRevisionHistory(pickedPackage, `current revision ${revision} does not have exactly one history entry`)
  }
  const current = matches[0]
  validatePackageRevision(pickedPackage, current, registry)
  if (
    pickedPackage.dist.integrity !== current.integrity ||
    normalizeRegistryUrl(pickedPackage.dist.tarball) !== normalizeRegistryUrl(current.tarball)
  ) {
    throw malformedRevisionHistory(pickedPackage, `revision ${revision} does not match the current artifact`)
  }
}

function validatePackageRevision (
  pickedPackage: PackageInRegistry,
  revision: PackageRevision,
  registry: string
): void {
  if (!isRevisionNumber(revision.revision)) {
    throw malformedRevisionHistory(pickedPackage, `revision ${String(revision.revision)} is not a canonical safe integer`)
  }
  if (
    typeof revision.integrity !== 'string' ||
    typeof revision.tarball !== 'string' ||
    !isIntegrityAddressedRegistryTarballUrl(normalizeRegistryUrl(revision.tarball), revision.integrity, registry)
  ) {
    throw malformedRevisionHistory(pickedPackage, `revision ${revision.revision} is not addressed by its complete sha512 integrity`)
  }
  if (revision.manifest == null || typeof revision.manifest !== 'object' || Array.isArray(revision.manifest)) {
    throw malformedRevisionHistory(pickedPackage, `revision ${revision.revision} has an invalid manifest`)
  }
}

function isRevisionNumber (revision: unknown): revision is number {
  return revision === 0 || isValidTarballRevision(revision)
}

function noMatchingRevision (pickedPackage: PackageInRegistry, wantedRevision: number): PnpmError {
  return new PnpmError('NO_MATCHING_REVISION',
    `No revision ${wantedRevision} is advertised for ${pickedPackage.name}@${pickedPackage.version}`)
}

function malformedRevisionHistory (pickedPackage: PackageInRegistry, reason: string): PnpmError {
  return new PnpmError('MALFORMED_METADATA',
    `The revision history for ${pickedPackage.name}@${pickedPackage.version} is invalid: ${reason}.`)
}
