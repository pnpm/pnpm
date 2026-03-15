import type { FetchFromRegistry } from '@pnpm/fetching-types'
import yaml from 'js-yaml'
import semver from 'semver'

export interface AquaRegistryPackage {
  type: string
  repo_owner: string
  repo_name: string
  description?: string
  asset?: string
  format?: string
  files?: AquaFile[]
  replacements?: Record<string, string>
  supported_envs?: string[]
  overrides?: AquaOverride[]
  checksum?: AquaChecksum
  version_constraint: string
  version_overrides?: AquaVersionOverride[]
}

export interface AquaVersionOverride {
  version_constraint: string
  asset?: string
  format?: string
  files?: AquaFile[]
  replacements?: Record<string, string>
  supported_envs?: string[]
  overrides?: AquaOverride[]
  checksum?: AquaChecksum
  rosetta2?: boolean
  windows_arm_emulation?: boolean
}

export interface AquaFile {
  name: string
  src?: string
}

export interface AquaOverride {
  goos?: string
  goarch?: string
  asset?: string
  format?: string
  files?: AquaFile[]
  replacements?: Record<string, string>
  checksum?: AquaChecksum | { enabled: false }
}

export interface AquaChecksum {
  type: string
  asset: string
  algorithm: string
  enabled?: boolean
}

interface AquaRegistryDocument {
  packages: AquaRegistryPackage[]
}

const AQUA_REGISTRY_BASE = 'https://raw.githubusercontent.com/aquaproj/aqua-registry/main/pkgs'

export async function fetchAquaRegistryPackage (
  fetchFromRegistry: FetchFromRegistry,
  owner: string,
  repo: string
): Promise<AquaRegistryPackage> {
  const url = `${AQUA_REGISTRY_BASE}/${encodeURIComponent(owner)}/${encodeURIComponent(repo)}/registry.yaml`
  const res = await fetchFromRegistry(url)
  if (!res.ok) {
    throw new Error(`Failed to fetch aqua registry for ${owner}/${repo}: ${res.status} ${res.statusText}`)
  }
  const text = await res.text()
  const doc = yaml.load(text) as AquaRegistryDocument
  return doc.packages[0]
}

export function findMatchingOverride (
  pkg: AquaRegistryPackage,
  version: string
): AquaVersionOverride | AquaRegistryPackage {
  // Strip leading 'v' for semver comparison
  const cleanVersion = version.startsWith('v') ? version.substring(1) : version

  if (pkg.version_overrides) {
    for (const override of pkg.version_overrides) {
      if (matchesVersionConstraint(override.version_constraint, cleanVersion, version)) {
        return override
      }
    }
  }

  // Fall back to base package if its constraint matches
  if (matchesVersionConstraint(pkg.version_constraint, cleanVersion, version)) {
    return pkg
  }

  // If nothing matches, return the last override (usually "true")
  if (pkg.version_overrides?.length) {
    return pkg.version_overrides[pkg.version_overrides.length - 1]
  }

  return pkg
}

function matchesVersionConstraint (
  constraint: string,
  cleanVersion: string,
  rawVersion: string
): boolean {
  if (constraint === 'true') return true
  if (constraint === 'false') return false

  // semver("<= X.Y.Z") or semver(">= X.Y.Z")
  const semverMatch = constraint.match(/^semver\("(.+)"\)$/)
  if (semverMatch) {
    return semver.satisfies(cleanVersion, semverMatch[1], { includePrerelease: true })
  }

  // Version == "X.Y.Z"
  const exactMatch = constraint.match(/^Version\s*==\s*"(.+)"$/)
  if (exactMatch) {
    return rawVersion === exactMatch[1] || cleanVersion === exactMatch[1]
  }

  return false
}
