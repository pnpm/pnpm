import { PnpmError } from '@pnpm/error'

export function assertUnpackedSize (unpackedSize: number, maxUnpackedSize?: number): void {
  if (maxUnpackedSize == null || unpackedSize <= maxUnpackedSize) return
  throw new PnpmError('TARBALL_UNPACKED_SIZE_EXCEEDED', `Tarball unpacks to more than ${maxUnpackedSize} bytes`, {
    hint: 'If the package is expected to be this large, raise the maxUnpackedSize setting.',
  })
}
