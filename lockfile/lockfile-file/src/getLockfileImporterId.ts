import path from 'node:path'
import normalize from 'normalize-path'

export function getLockfileImporterId(
  lockfileDir: string,
  prefix: string
): string {
  return normalize(path.relative(lockfileDir, prefix)) || '.'
}
