import { updateWorkspaceManifest } from '@pnpm/workspace.workspace-manifest-writer'

import {
  type PolicyHandlersOptions,
  type PolicyViolation,
  setupPolicyHandlers,
} from './policyHandlers.js'

export interface GlobalPolicyCallbacks {
  handleResolutionPolicyViolations?: (violations: readonly PolicyViolation[]) => Promise<void>
  updateResolutionPolicyManifest?: (violations: readonly PolicyViolation[], dir: string) => Promise<void>
}

export function createGlobalPolicyCallbacks (opts: PolicyHandlersOptions): GlobalPolicyCallbacks {
  const policyHandlers = setupPolicyHandlers(opts)
  if (policyHandlers == null) return {}
  return {
    handleResolutionPolicyViolations: policyHandlers.handleResolutionPolicyViolations,
    updateResolutionPolicyManifest: async (violations, dir) => {
      const policyUpdates = policyHandlers.pickManifestUpdates(violations)
      if (policyUpdates != null) {
        await updateWorkspaceManifest(dir, policyUpdates)
      }
    },
  }
}
