import audit from '@pnpm/audit'
import { WANTED_LOCKFILE } from '@pnpm/constants'
import PnpmError from '@pnpm/error'
import { readWantedLockfile } from '@pnpm/lockfile-file'
import { table } from 'table'
import { PnpmOptions } from '../types'

export default async function (
  args: string[],
  opts: PnpmOptions & {
    json?: boolean,
  },
  command: string,
) {
  const lockfile = await readWantedLockfile(opts.lockfileDir || opts.dir, { ignoreIncompatible: true })
  if (!lockfile) {
    throw new PnpmError('AUDIT_NO_LOCKFILE', `No ${WANTED_LOCKFILE} found: Cannot audit a project without a lockfile`)
  }
  const auditReport = await audit(lockfile, { registry: opts.registries.default })
  if (opts.json) {
    return JSON.stringify(auditReport, null, 2)
  }

  let output = ''
  for (const advisory of Object.values(auditReport.advisories)) {
    output += table([
      [advisory.severity, advisory.title],
      ['Package', advisory.module_name],
      ['Vulnerable versions', advisory.vulnerable_versions],
      ['Patched versions', advisory.patched_versions],
    ])
  }
  return output
}
