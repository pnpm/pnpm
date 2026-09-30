import { PnpmError } from '@pnpm/error'

import {
  type AccessOptions,
  createJsonWriteHeaders,
  createPackageRequestContext,
  escapePackageName,
  throwRegistryError,
} from './accessRequest.js'
import { normalizeRegistryUrl } from './common.js'

type NormalizedAccess = 'public' | 'restricted'

export async function setStatus (
  opts: AccessOptions,
  params: string[]
): Promise<string> {
  const normalizedAccess = parseAccessStatus(params)
  const packageName = params[1]
  if (!packageName) {
    throw new PnpmError('ACCESS_SET_STATUS_PACKAGE_REQUIRED', 'Package name is required (e.g., pnpm access set status=public @scope/pkg)')
  }

  if (!packageName.startsWith('@')) {
    throw new PnpmError('ACCESS_SET_STATUS_UNSCOPED', 'Access settings can only be changed for scoped packages (@scope/name). Unscoped packages are always public.')
  }

  const { registryUrl, authHeader, fetchFromRegistry } = createPackageRequestContext(opts, packageName)
  const otp = opts.cliOptions?.otp

  const accessUrl = new URL(`-/package/${escapePackageName(packageName)}/access`, normalizeRegistryUrl(registryUrl)).href
  const response = await fetchFromRegistry(accessUrl, {
    authHeaderValue: authHeader,
    method: 'POST',
    headers: createJsonWriteHeaders(otp),
    body: JSON.stringify({ access: normalizedAccess }),
  })

  if (!response.ok) {
    await throwRegistryError(response, `set access to "${normalizedAccess}" for`)
  }

  return `${packageName}: ${normalizedAccess === 'public' ? 'public' : 'restricted'}`
}

function parseAccessStatus (params: string[]): NormalizedAccess {
  if (params.length === 0 || !params[0].startsWith('status=')) {
    throw new PnpmError('ACCESS_SET_STATUS_REQUIRED', 'Package visibility is required (e.g., pnpm access set status=public @scope/pkg)')
  }

  const accessValue = params[0].slice('status='.length)
  if (accessValue !== 'public' && accessValue !== 'private' && accessValue !== 'restricted') {
    throw new PnpmError('ACCESS_SET_STATUS_INVALID', `Invalid access value "${accessValue}". Must be "public" or "private".`)
  }

  return accessValue === 'private' || accessValue === 'restricted' ? 'restricted' : 'public'
}

export async function setMfa (
  opts: AccessOptions,
  params: string[]
): Promise<string> {
  if (params.length === 0 || !params[0].startsWith('mfa=')) {
    throw new PnpmError('ACCESS_SET_MFA_REQUIRED', 'MFA level is required (e.g., pnpm access set mfa=automation @scope/pkg)')
  }

  const mfaValue = params[0].slice('mfa='.length)
  if (!['none', 'publish', 'automation'].includes(mfaValue)) {
    throw new PnpmError('ACCESS_SET_MFA_INVALID', `Invalid MFA value "${mfaValue}". Must be "none", "publish", or "automation".`)
  }

  const packageName = params[1]
  if (!packageName) {
    throw new PnpmError('ACCESS_SET_MFA_PACKAGE_REQUIRED', 'Package name is required (e.g., pnpm access set mfa=automation @scope/pkg)')
  }

  const { registryUrl, authHeader, fetchFromRegistry } = createPackageRequestContext(opts, packageName)
  const otp = opts.cliOptions?.otp

  const accessUrl = new URL(`-/package/${escapePackageName(packageName)}/access`, normalizeRegistryUrl(registryUrl)).href
  const publishRequiresTfa = mfaValue !== 'none'

  const response = await fetchFromRegistry(accessUrl, {
    authHeaderValue: authHeader,
    method: 'POST',
    headers: createJsonWriteHeaders(otp),
    body: JSON.stringify({ publish_requires_tfa: publishRequiresTfa }),
  })

  if (!response.ok) {
    await throwRegistryError(response, 'set MFA for')
  }

  return `${packageName}: mfa=${mfaValue}`
}

export async function grantAccess (
  opts: AccessOptions,
  params: string[]
): Promise<string> {
  if (params.length < 2) {
    throw new PnpmError('ACCESS_GRANT_ARGS_REQUIRED', 'Permissions and scope:team are required (e.g., pnpm access grant read-only @scope:developers @scope/pkg)')
  }

  const permissions = params[0]
  if (permissions !== 'read-only' && permissions !== 'read-write') {
    throw new PnpmError('ACCESS_GRANT_INVALID_PERMISSIONS', `Invalid permissions "${permissions}". Must be "read-only" or "read-write".`)
  }

  const scopeTeam = params[1]
  if (!scopeTeam.includes(':')) {
    throw new PnpmError('ACCESS_GRANT_INVALID_TEAM', `Invalid team "${scopeTeam}". Format must be "scope:team".`)
  }

  const packageName = params[2]
  if (!packageName) {
    throw new PnpmError('ACCESS_GRANT_PACKAGE_REQUIRED', 'Package name is required (e.g., pnpm access grant read-only @scope:developers @scope/pkg)')
  }

  const { registryUrl, authHeader, fetchFromRegistry } = createPackageRequestContext(opts, packageName)
  const otp = opts.cliOptions?.otp

  const grantUrl = new URL(buildTeamPackagesPath(scopeTeam), normalizeRegistryUrl(registryUrl)).href
  const response = await fetchFromRegistry(grantUrl, {
    authHeaderValue: authHeader,
    method: 'PUT',
    headers: createJsonWriteHeaders(otp),
    body: JSON.stringify({ package: packageName, permissions }),
  })

  if (!response.ok) {
    await throwRegistryError(response, `grant ${permissions} access for ${scopeTeam} on`)
  }

  return `+${scopeTeam} (${permissions}): ${packageName}`
}

export async function revokeAccess (
  opts: AccessOptions,
  params: string[]
): Promise<string> {
  if (params.length < 1) {
    throw new PnpmError('ACCESS_REVOKE_ARGS_REQUIRED', 'scope:team and package name are required (e.g., pnpm access revoke @scope:developers @scope/pkg)')
  }

  const scopeTeam = params[0]
  if (!scopeTeam.includes(':')) {
    throw new PnpmError('ACCESS_REVOKE_INVALID_TEAM', `Invalid team "${scopeTeam}". Format must be "scope:team".`)
  }

  const packageName = params[1]
  if (!packageName) {
    throw new PnpmError('ACCESS_REVOKE_PACKAGE_REQUIRED', 'Package name is required (e.g., pnpm access revoke @scope:developers @scope/pkg)')
  }

  const { registryUrl, authHeader, fetchFromRegistry } = createPackageRequestContext(opts, packageName)
  const otp = opts.cliOptions?.otp

  const revokeUrl = new URL(buildTeamPackagesPath(scopeTeam), normalizeRegistryUrl(registryUrl)).href
  const response = await fetchFromRegistry(revokeUrl, {
    authHeaderValue: authHeader,
    method: 'DELETE',
    headers: createJsonWriteHeaders(otp),
    body: JSON.stringify({ package: packageName }),
  })

  if (!response.ok) {
    await throwRegistryError(response, `revoke ${scopeTeam}'s access to`)
  }

  return `-${scopeTeam}: ${packageName}`
}

function buildTeamPackagesPath (scopeTeam: string): string {
  const [scope, team] = scopeTeam.split(':')
  return `-/team/${encodeURIComponent(scope.startsWith('@') ? scope.slice(1) : scope)}/${encodeURIComponent(team)}/package`
}
