import path from 'node:path'
import { stripVTControlCharacters } from 'node:util'

import { logger } from '@pnpm/logger'
import { sanitizeInline } from '@pnpm/text.sanitize'
import type { ProjectManifest } from '@pnpm/types'

/**
 * Warns when a package depended on through `link:` declares peer
 * dependencies. A `link:` dependency is a symlink, so Node resolves the
 * package's imports from its own directory and never sees the peers installed
 * in the project that links it.
 */
export function warnAboutLinkedPeerDependencies (
  manifest: ProjectManifest | null | undefined,
  opts: { pkgDir: string, prefix: string }
): void {
  if (!manifest?.peerDependencies || Object.keys(manifest.peerDependencies).length === 0) return
  const packageName = sanitizeWarningText(manifest.name ?? path.basename(opts.pkgDir))
  const peerDeps = Object.entries(manifest.peerDependencies)
    .map(([key, value]) => `  - ${sanitizeWarningText(key)}@${sanitizeWarningText(typeof value === 'string' ? value : JSON.stringify(value))}`)
    .join(', ')

  logger.warn({
    message: `The package ${packageName}, which you have just pnpm linked, has the following peerDependencies specified in its package.json:

${peerDeps}

The linked in dependency will not resolve the peer dependencies from the target node_modules.
This might cause issues in your project. To resolve this, you may use the "file:" protocol to reference the local dependency.`,
    prefix: opts.prefix,
  })
}

function sanitizeWarningText (text: string): string {
  return sanitizeInline(stripVTControlCharacters(text))
}
