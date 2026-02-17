import crypto from 'crypto'
import { parseDepPath } from '@pnpm/dependency-path'

export function peersSuffixHashFromDepPath (depPath: string): string | undefined {
  const { peerDepGraphHash } = parseDepPath(depPath)
  if (!peerDepGraphHash) return undefined
  return crypto.createHash('sha256').update(peerDepGraphHash).digest('hex').slice(0, 4)
}
