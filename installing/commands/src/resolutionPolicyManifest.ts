import { updateWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-writer'

import type { PolicyHandlersPlan, PolicyViolation } from './policyHandlers.js'

export function createResolutionPolicyManifestUpdater (
  policyHandlers: PolicyHandlersPlan | undefined
): ((violations: readonly PolicyViolation[], dir: string) => Promise<void>) | undefined {
  if (policyHandlers == null) return undefined
  return async (violations, dir) => {
    const policyUpdates = policyHandlers.pickManifestUpdates(violations)
    if (policyUpdates != null) {
      await updateWorkspaceManifest(dir, policyUpdates)
    }
  }
}
