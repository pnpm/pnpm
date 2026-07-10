import type { LicensesConfig } from '@pnpm/types'

export type NormalizedPolicy = LicensesConfig & { mode: 'strict' | 'loose' }

export function resolveLicensePolicy (licenses?: LicensesConfig | null): NormalizedPolicy | null {
  if (licenses == null || licenses.mode === 'none') {
    return null
  }
  const hasLists =
    (licenses.allowed?.length ?? 0) > 0 ||
    (licenses.disallowed?.length ?? 0) > 0 ||
    (licenses.overrides != null && Object.keys(licenses.overrides).length > 0)
  if (!hasLists) {
    return null
  }
  return { ...licenses, mode: licenses.mode ?? 'loose' }
}
