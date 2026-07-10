import type { LicensesConfig } from '@pnpm/types'

import { resolveLicensePolicy } from './policy.js'
import { extractLicenseIds } from './spdxMatcher.js'

export function shouldRunLicenseCheck (licenses?: LicensesConfig | null): boolean {
  return resolveLicensePolicy(licenses) != null
}

export interface NormalizedLicenseArgs {
  /** License IDs to add to the policy list */
  ids: string[]
  /** Expressions that were expanded to leaf IDs */
  expanded: string[]
  /** Strings that could not be parsed as SPDX */
  unrecognized: string[]
}

/**
 * Normalizes license arguments for allow/disallow commands.
 * - Simple IDs (e.g. "MIT") are kept as-is
 * - WITH expressions (e.g. "Apache-2.0 WITH LLVM-exception") are kept as-is
 * - Plus expressions (e.g. "GPL-2.0+") are kept as-is
 * - Compound expressions (e.g. "MIT OR Apache-2.0") are expanded to leaf IDs
 * - Non-SPDX strings are kept as-is (for literal matching)
 */
export function normalizeLicenseArgs (args: string[]): NormalizedLicenseArgs {
  const ids: string[] = []
  const expanded: string[] = []
  const unrecognized: string[] = []

  for (const rawArg of args) {
    const arg = rawArg.trim()
    if (arg.length === 0) continue
    const extractedIds = extractLicenseIds(arg)
    if (extractedIds.length === 0) {
      // Non-SPDX string — keep for literal matching
      unrecognized.push(arg)
      ids.push(arg)
    } else if (extractedIds.length === 1) {
      // Keep the original form only for WITH/plus expressions where the
      // literal matters for policy matching (e.g. "GPL-2.0+" or
      // "Apache-2.0 WITH LLVM-exception"). Strip outer parentheses first
      // to ensure the stored form matches what the evaluator checks.
      // Otherwise use the extracted ID.
      const normalized = stripOuterParens(arg)
      if (/\bWITH\b/.test(normalized) || normalized.endsWith('+')) {
        ids.push(normalized)
      } else {
        ids.push(extractedIds[0])
      }
    } else {
      // Compound expression — expand to leaf IDs
      expanded.push(arg)
      for (const id of extractedIds) {
        ids.push(id)
      }
    }
  }

  return { ids: [...new Set(ids)], expanded, unrecognized }
}

function stripOuterParens (s: string): string {
  while (s.startsWith('(') && s.endsWith(')')) {
    s = s.slice(1, -1).trim()
  }
  return s
}
