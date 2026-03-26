import { execSync } from 'node:child_process'
import { globalWarn } from '@pnpm/logger'

/**
 * Remove macOS Gatekeeper quarantine extended attribute from a file.
 * 
 * Background: When pnpm copies files from its content-addressable store to node_modules,
 * macOS preserves extended attributes including com.apple.quarantine. This xattr can
 * be present on store blobs if packages were initially installed under a Gatekeeper-enabled
 * application (e.g., a Git client with LSFileQuarantineEnabled=YES).
 * 
 * After pnpm verifies file integrity against the lockfile hash, there's no security
 * reason to retain the quarantine xattr. Native binaries (.node files) with quarantine
 * trigger Gatekeeper dialogs blocking execution even when integrity is verified.
 * 
 * This function removes the quarantine xattr after file copy, matching Homebrew's
 * strategy for downloaded artifacts after checksum verification.
 * 
 * @param filePath - Absolute path to the file
 * @returns true if quarantine was removed or didn't exist, false if removal failed
 */
export function removeQuarantine (filePath: string): boolean {
  // Only run on macOS
  if (process.platform !== 'darwin') {
    return true
  }

  try {
    // Remove com.apple.quarantine xattr
    // 2>/dev/null suppresses "No such xattr" errors when quarantine isn't present
    // This is expected and not an error condition
    execSync(`/usr/bin/xattr -d com.apple.quarantine "${filePath}" 2>/dev/null`, {
      encoding: 'utf8',
      stdio: 'pipe',
    })
    return true
  } catch (err: unknown) {
    // Exit code 1 means "xattr not found" - this is success (nothing to remove)
    if (typeof err === 'object' && err !== null && 'status' in err && err.status === 1) {
      return true
    }
    
    // Any other error (permissions, file not found, etc.) is unexpected but non-fatal
    // Log warning but don't fail the installation
    globalWarn(`Failed to remove quarantine xattr from ${filePath}: ${err instanceof Error ? err.message : String(err)}`)
    return false
  }
}

/**
 * Check if a file has the quarantine extended attribute (for testing/debugging).
 * 
 * @param filePath - Absolute path to the file
 * @returns true if file has quarantine xattr, false otherwise
 */
export function hasQuarantine (filePath: string): boolean {
  if (process.platform !== 'darwin') {
    return false
  }

  try {
    const output = execSync(`/usr/bin/xattr -l "${filePath}" 2>/dev/null`, {
      encoding: 'utf8',
      stdio: 'pipe',
    })
    return output.includes('com.apple.quarantine')
  } catch {
    return false
  }
}
