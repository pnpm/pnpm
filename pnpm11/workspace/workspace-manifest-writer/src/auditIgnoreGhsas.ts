import type { WorkspaceManifest } from '@pnpm/workspace.workspace-manifest-reader'
import { equals } from 'ramda'

type AuditSettings = NonNullable<WorkspaceManifest['audit']>

/**
 * Set the audit ignore list to `ghsas` (the complete desired list) in
 * whichever spelling the manifest uses — the canonical `audit.ignore` wins
 * over the deprecated `auditConfig.ignoreGhsas`, matching the reader's
 * precedence, so a stale canonical list can't shadow the update on the next
 * read. When both spellings are present, the shadowed deprecated list is
 * removed as part of the write. `auditConfig.ignoreGhsas` is created when
 * neither is present. An empty `ghsas` removes the list, dropping its parent
 * block when nothing else remains in it. Returns whether anything changed.
 */
export function setAuditIgnoreGhsas (manifest: Partial<WorkspaceManifest>, ghsas: string[]): boolean {
  if (manifest.audit?.ignore != null) {
    let changed = setCanonicalAuditIgnore(manifest, manifest.audit, ghsas)
    if (manifest.auditConfig?.ignoreGhsas != null) {
      changed = removeAuditConfigIgnoreGhsas(manifest) || changed
    }
    return changed
  }
  if (ghsas.length === 0) {
    return removeAuditConfigIgnoreGhsas(manifest)
  }
  if (equals(manifest.auditConfig?.ignoreGhsas, ghsas)) {
    return false
  }
  manifest.auditConfig = { ...manifest.auditConfig, ignoreGhsas: ghsas }
  return true
}

function setCanonicalAuditIgnore (manifest: Partial<WorkspaceManifest>, audit: AuditSettings, ghsas: string[]): boolean {
  if (ghsas.length === 0) {
    delete audit.ignore
    if (Object.keys(audit).length === 0) {
      delete manifest.audit
    }
    return true
  }
  if (equals(audit.ignore, ghsas)) {
    return false
  }
  audit.ignore = ghsas
  return true
}

function removeAuditConfigIgnoreGhsas (manifest: Partial<WorkspaceManifest>): boolean {
  if (manifest.auditConfig?.ignoreGhsas == null) {
    return false
  }
  delete manifest.auditConfig.ignoreGhsas
  if (Object.keys(manifest.auditConfig).length === 0) {
    delete manifest.auditConfig
  }
  return true
}
