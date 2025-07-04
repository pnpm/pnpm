import { type ProjectId } from '@pnpm/types'
import path from 'node:path'
import normalize from 'normalize-path'

export function getLockfileImporterId (lockfileDir: string, prefix: string): ProjectId {
  return (normalize(path.relative(lockfileDir, prefix)) || '.') as ProjectId
}
