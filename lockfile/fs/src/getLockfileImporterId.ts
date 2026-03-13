import path from 'path'

import type { ProjectId } from '@pnpm/types'
import normalize from 'normalize-path'

export function getLockfileImporterId (lockfileDir: string, prefix: string): ProjectId {
  return (normalize(path.relative(lockfileDir, prefix)) || '.') as ProjectId
}
