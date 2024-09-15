import crypto from 'crypto'

export function createShortHash (input: string): string {
  const hash = crypto.createHash('sha256')
  hash.update(input)
  return hash.digest('hex').substring(0, 32)
}
