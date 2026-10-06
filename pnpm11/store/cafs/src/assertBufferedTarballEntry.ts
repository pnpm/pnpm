import { inspect } from 'node:util'

import { PnpmError } from '@pnpm/error'

export function assertBufferedTarballEntry (entry: { fileName: string, size: number }, maxBufferedEntrySize?: number): void {
  if (maxBufferedEntrySize == null || entry.size <= maxBufferedEntrySize) return
  throw new PnpmError('TARBALL_ENTRY_TOO_LARGE', `Tarball entry ${inspect(entry.fileName, { colors: false })} requires buffering ${entry.size} bytes, exceeding the ${maxBufferedEntrySize}-byte limit`)
}
