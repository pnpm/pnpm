import fs from 'node:fs'
import path from 'node:path'

import { isError } from '@pnpm/error'

/**
 * Anchors a relative `lockfileDir` setting to `anchorDir` by string
 * concatenation without collapsing `..` segments, so symlink-parent
 * containment checks can inspect every segment before traversal.
 */
export function anchorRawLockfileDir (anchorDir: string, rawSpec: string): string {
  const nativeSpec = path.sep === '\\' ? rawSpec.replaceAll('/', '\\') : rawSpec
  if (path.parse(nativeSpec).root !== '') {
    return nativeSpec
  }
  const resolvedAnchor = tryRealpathSync(anchorDir) ?? anchorDir
  const prefix = resolvedAnchor.endsWith(path.sep) ? resolvedAnchor : `${resolvedAnchor}${path.sep}`
  return `${prefix}${nativeSpec}`
}

function tryRealpathSync (targetPath: string): string | undefined {
  try {
    return fs.realpathSync(targetPath)
  } catch (err: unknown) {
    if (
      isError(err) &&
      'code' in err &&
      (err.code === 'ENOENT' || err.code === 'ENOTDIR' || err.code === 'ELOOP')
    ) {
      return undefined
    }
    throw err
  }
}
