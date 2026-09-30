import { pickRegistryForPackage } from '@pnpm/config.pick-registry-for-package'
import { PnpmError } from '@pnpm/error'
import { createGetAuthHeaderByURI } from '@pnpm/network.auth-header'
import { createFetchFromRegistry, type CreateFetchFromRegistryOptions, type FetchFromRegistry } from '@pnpm/network.fetch'
import npa from '@pnpm/npm-package-arg'
import type { RegistriesByScope, RegistryConfig } from '@pnpm/types'

import { readErrorBody } from './common.js'

export const DEFAULT_REGISTRY_URL = 'https://registry.npmjs.org/'

export interface AccessOptions extends CreateFetchFromRegistryOptions {
  cliOptions?: {
    json?: boolean
    otp?: string
  }
  configByUri?: Record<string, RegistryConfig>
  registriesByScope?: RegistriesByScope
}

export interface PackageRequestContext {
  registryUrl: string
  authHeader: string | undefined
  fetchFromRegistry: FetchFromRegistry
}

export function getRegistries (opts: AccessOptions): RegistriesByScope {
  return opts.registriesByScope ?? { default: DEFAULT_REGISTRY_URL }
}

export function createPackageRequestContext (opts: AccessOptions, packageName: string): PackageRequestContext {
  const registryUrl = pickRegistryForPackage(getRegistries(opts), packageName)
  return {
    registryUrl,
    authHeader: getAuthHeaderForRegistry(opts.configByUri, registryUrl, packageName),
    fetchFromRegistry: createFetchFromRegistry(opts),
  }
}

export function createJsonWriteHeaders (otp: string | undefined): Record<string, string> {
  return {
    'content-type': 'application/json',
    ...(otp ? { 'npm-otp': otp } : {}),
  }
}

export function getAuthHeaderForRegistry (
  configByUri: Record<string, RegistryConfig> | undefined,
  registryUrl: string,
  packageName?: string
): string | undefined {
  const getAuthHeader = createGetAuthHeaderByURI(configByUri ?? {})
  return getAuthHeader(registryUrl, packageName ? { pkgName: packageName } : undefined)
}

export function escapePackageName (packageName: string): string {
  let parsed
  try {
    parsed = npa(packageName)
  } catch {
    throw new PnpmError('ACCESS_INVALID_PACKAGE_NAME', `Invalid package name "${packageName}"`)
  }
  return parsed.escapedName ?? encodeURIComponent(packageName).replace(/^%40/, '@')
}

export async function throwRegistryError (response: Response, action: string): Promise<never> {
  const errorBody = sanitize(await readErrorBody(response))
  if (response.status === 401) {
    throw new PnpmError('UNAUTHORIZED', `You must be logged in to ${action} packages. ${errorBody}`)
  }
  if (response.status === 403) {
    throw new PnpmError('FORBIDDEN', `You do not have permission to ${action} this package. ${errorBody}`)
  }
  if (response.status === 404) {
    throw new PnpmError('PACKAGE_NOT_FOUND', `Package not found in registry. ${errorBody}`)
  }
  if (response.status === 422) {
    throw new PnpmError('ACCESS_VALIDATION_ERROR', `Invalid request: ${errorBody}`)
  }
  throw new PnpmError('REGISTRY_ERROR', `Failed to ${action} package: ${response.status} ${response.statusText}. ${errorBody}`)
}

function sanitize (text: string): string {
  let result = ''
  for (let charIndex = 0; charIndex < text.length; charIndex++) {
    const code = text.charCodeAt(charIndex)
    if ((code > 31 && code !== 127) || code === 9 || code === 10) {
      result += text[charIndex]
    }
  }
  return result
}
