import { globalWarn } from '@pnpm/logger'
import { PnpmError } from '@pnpm/error'
import {
  type ProjectManifest,
} from '@pnpm/types'
import semver from 'semver'

export interface PackageManagerValidationResult {
  isValid: boolean
  error?: string
  warnings: string[]
}

interface ParsedPackageManager {
  name: string
  version?: string
}

function parsePackageManagerField (packageManager: string): ParsedPackageManager {
  if (!packageManager.includes('@')) {
    return { name: packageManager, version: undefined }
  }
  const lastAtIndex = packageManager.lastIndexOf('@')
  if (lastAtIndex === 0) {
    return { name: packageManager, version: undefined }
  }
  const name = packageManager.substring(0, lastAtIndex)
  const versionPart = packageManager.substring(lastAtIndex + 1)
  const [version] = versionPart.split('+')
  return { name, version }
}

export function validatePackageManagerConsistency (
  manifest: ProjectManifest
): PackageManagerValidationResult {
  const warnings: string[] = []

  const packageManagerField = manifest.packageManager
    ? parsePackageManagerField(manifest.packageManager)
    : undefined

  const devEnginesPMs = manifest.devEngines?.packageManager
    ? (Array.isArray(manifest.devEngines.packageManager)
      ? manifest.devEngines.packageManager
      : [manifest.devEngines.packageManager])
    : []

  if (!packageManagerField && devEnginesPMs.length === 0) {
    return { isValid: true, warnings: [] }
  }

  if (packageManagerField && devEnginesPMs.length === 0) {
    return { isValid: true, warnings: [] }
  }

  if (!packageManagerField && devEnginesPMs.length > 0) {
    return { isValid: true, warnings: [] }
  }

  const matchingDevEngine = devEnginesPMs.find(
    de => de.name === packageManagerField!.name
  )

  if (!matchingDevEngine) {
    return {
      isValid: false,
      error:
        `"packageManager" field is set to "${packageManagerField!.name}@${packageManagerField!.version}" ` +
        'which does not match the value defined in "devEngines.packageManager" field',
      warnings: [],
    }
  }

  return { isValid: true, warnings }
}

const shownWarnings = new Set<string>()

export function validateAndWarnPackageManagerConsistency (manifest: ProjectManifest): void {
  const result = validatePackageManagerConsistency(manifest)

  if (!result.isValid) {
    throw new PnpmError('ERR_PNPM_PACKAGE_MANAGER_CONFLICT', result.error!)
  }

  for (const warning of result.warnings) {
    if (!shownWarnings.has(warning)) {
      shownWarnings.add(warning)
      globalWarn(warning)
    }
  }
}
