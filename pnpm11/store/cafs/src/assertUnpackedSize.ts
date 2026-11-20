import { PnpmError } from '@pnpm/error'

/** The default of the `maxTarballUnpackedSize` setting, for callers that pass none. */
export const DEFAULT_MAX_TARBALL_UNPACKED_SIZE = 4294967296 // 4 GiB

export function assertUnpackedSize (unpackedSize: number, maxTarballUnpackedSize?: number): void {
  if (maxTarballUnpackedSize == null || unpackedSize <= maxTarballUnpackedSize) return
  throw new PnpmError('TARBALL_TOO_LARGE', `The archive is larger than pnpm accepts: its files total more than ${maxTarballUnpackedSize} bytes`, {
    hint: 'If the package is legitimately this large, raise the maxTarballUnpackedSize setting.',
  })
}
