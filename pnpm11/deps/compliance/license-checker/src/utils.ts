import type { LicensesConfig } from '@pnpm/types'

import { resolveLicensePolicy } from './policy.js'

export function shouldRunLicenseCheck (licenses?: LicensesConfig | null): boolean {
  return resolveLicensePolicy(licenses) != null
}
