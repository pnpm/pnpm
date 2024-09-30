import crypto from 'crypto'
import sortKeys from 'sort-keys'

export function createObjectChecksum (obj: Record<string, unknown>): string {
  const s = JSON.stringify(sortKeys(obj, { deep: true }))
  return crypto.createHash('md5').update(s).digest('hex')
}
