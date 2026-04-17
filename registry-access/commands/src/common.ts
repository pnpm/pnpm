import { types as allTypes } from '@pnpm/config.reader'
import { PnpmError } from '@pnpm/error'
import npa from '@pnpm/npm-package-arg'
import { pick } from 'ramda'

export function rcOptionsTypes (): Record<string, unknown> {
  return pick([
    'registry',
  ], allTypes)
}

export function parsePackageSpec (spec: string): { name: string, escapedName: string, versionRange: string | undefined } {
  let parsed: ReturnType<typeof npa>
  try {
    parsed = npa(spec)
  } catch {
    throw new PnpmError('INVALID_PACKAGE_SPEC', `Invalid package spec: ${spec}`)
  }
  if (!parsed.name || !parsed.escapedName) {
    throw new PnpmError('INVALID_PACKAGE_SPEC', `Invalid package spec: ${spec}`)
  }
  const versionRange = parsed.rawSpec || undefined
  return { name: parsed.name, escapedName: parsed.escapedName, versionRange }
}

export function normalizeRegistryUrl (registryUrl: string): string {
  return registryUrl.endsWith('/') ? registryUrl : `${registryUrl}/`
}
