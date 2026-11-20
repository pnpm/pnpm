import crypto from 'node:crypto'

export function hashBuffer (algorithm: string, buffer: Uint8Array): string {
  return crypto.createHash(algorithm).update(buffer).digest('hex')
}
