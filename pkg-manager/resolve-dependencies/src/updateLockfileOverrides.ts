import { type ProjectSnapshot } from '@pnpm/lockfile.types'

/**
 * Create a new overrides object based on an existing overrides.
 * All the references in the new overrides would be corrected by updating from the specifiers of the root snapshot.
 */
export function updateLockfileOverrides (
  existingOverrides: Record<string, string> | undefined,
  rootSnapshot: ProjectSnapshot | undefined,
  overridesRefMap: Record<string, string | undefined> | undefined
): Record<string, string> | undefined {
  if (!existingOverrides || !rootSnapshot || !overridesRefMap) return existingOverrides

  const overrides = { ...existingOverrides }
  for (const [dep, refTarget] of Object.entries(overridesRefMap)) {
    if (!refTarget) continue
    const spec: string | undefined = rootSnapshot.specifiers[refTarget]
    if (spec) {
      overrides[dep] = spec
    }
  }
  return overrides
}
