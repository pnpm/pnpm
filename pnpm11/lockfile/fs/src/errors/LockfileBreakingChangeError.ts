import { PnpmError } from '@pnpm/error'

export class LockfileBreakingChangeError extends PnpmError {
  public filename: string
  constructor (
    filename: string,
    opts?: {
      lockfileVersion?: string
      wantedVersions?: string[]
    }
  ) {
    super('LOCKFILE_BREAKING_CHANGE', formatMessage(filename, opts))
    this.filename = filename
  }
}

function formatMessage (
  filename: string,
  opts?: {
    lockfileVersion?: string
    wantedVersions?: string[]
  }
): string {
  if (opts?.lockfileVersion == null) {
    return `Lockfile ${filename} not compatible with current pnpm`
  }
  const wantedVersions = opts.wantedVersions == null || opts.wantedVersions.length === 0
    ? ''
    : `, but the current pnpm version supports lockfileVersion ${opts.wantedVersions.join(', ')}`
  return `Lockfile ${filename} not compatible with current pnpm: it was generated with lockfileVersion ${opts.lockfileVersion}${wantedVersions}`
}
