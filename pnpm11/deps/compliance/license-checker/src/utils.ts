import type { LicensesConfig, ProjectManifest } from '@pnpm/types'

import { extractLicenseIds } from './spdxMatcher.js'

export interface IncludeFlags {
  dev?: boolean
  production?: boolean
  optional?: boolean
}

export function resolveInclude (
  environment: NonNullable<LicensesConfig['environment']>,
  opts?: IncludeFlags
): { dependencies: boolean, devDependencies: boolean, optionalDependencies: boolean } {
  if (environment === 'prod') {
    return {
      dependencies: true,
      devDependencies: false,
      optionalDependencies: opts?.optional !== false,
    }
  }
  if (environment === 'dev') {
    return {
      dependencies: false,
      devDependencies: true,
      optionalDependencies: false,
    }
  }
  return {
    dependencies: opts?.production !== false,
    devDependencies: opts?.dev !== false,
    optionalDependencies: opts?.optional !== false,
  }
}

export function collectDirectDeps (
  manifest: ProjectManifest,
  selectedProjectsGraph?: Record<string, { package: { manifest: ProjectManifest } }>
): Set<string> {
  const manifests: ProjectManifest[] = [manifest]
  if (selectedProjectsGraph) {
    for (const project of Object.values(selectedProjectsGraph)) {
      manifests.push(project.package.manifest)
    }
  }
  const deps = new Set<string>()
  for (const m of manifests) {
    for (const name of Object.keys(m.dependencies ?? {})) deps.add(name)
    for (const name of Object.keys(m.devDependencies ?? {})) deps.add(name)
    for (const name of Object.keys(m.optionalDependencies ?? {})) deps.add(name)
  }
  return deps
}

export function shouldRunLicenseCheck (licenses?: LicensesConfig | null): boolean {
  const mode = licenses?.mode
  return mode === 'strict' || mode === 'loose'
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

  for (const arg of args) {
    const extractedIds = extractLicenseIds(arg)
    if (extractedIds.length === 0) {
      // Non-SPDX string — keep for literal matching
      unrecognized.push(arg)
      ids.push(arg)
    } else if (extractedIds.length === 1) {
      // Simple ID, WITH expression, or plus — keep the original argument
      // so "GPL-2.0+" and "Apache-2.0 WITH LLVM-exception" are preserved
      ids.push(arg)
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
