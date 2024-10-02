import crypto from 'crypto'
import sortKeys from 'sort-keys'
import isEmpty from 'ramda/src/isEmpty'

export function createObjectChecksum (obj: Record<string, unknown> | undefined): string | undefined {
  if (!obj || isEmpty(obj)) return undefined
  const s = JSON.stringify(sortKeys(obj, { deep: true }))
  return crypto.createHash('md5').update(s).digest('hex')
}
