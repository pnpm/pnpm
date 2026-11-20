import { isError, PnpmError } from '@pnpm/error'
import ssri from 'ssri'

interface RegistryDist {
  integrity?: string
  shasum: string
  tarball: string
}

/**
 * The integrity a registry version pins its tarball with, which is also the
 * integrity the store keys that tarball under: `dist.integrity`, or the
 * `sha1-` form of a legacy `dist.shasum`.
 */
export function getIntegrity (dist: RegistryDist): string | undefined {
  if (dist.integrity) {
    return dist.integrity
  }
  if (!dist.shasum) {
    return undefined
  }
  const integrity = ssri.fromHex(dist.shasum, 'sha1')
  if (!integrity) {
    throw new PnpmError('INVALID_TARBALL_INTEGRITY', `Tarball "${dist.tarball}" has invalid shasum specified in its metadata: ${dist.shasum}`)
  }
  return integrity.toString()
}

/**
 * {@link getIntegrity} for a store lookup. A shasum that is not a hex digest
 * pins nothing the store could hold, so it reads as `undefined` instead of
 * failing a pick over a version the pick may never select.
 */
export function getStoreIntegrity (dist: RegistryDist): string | undefined {
  try {
    return getIntegrity(dist)
  } catch (err: unknown) {
    if (isError(err) && 'code' in err && err.code === 'ERR_PNPM_INVALID_TARBALL_INTEGRITY') {
      return undefined
    }
    throw err
  }
}
