import { hasher } from 'node-object-hash'

export const hashObjectWithoutSorting = hasher({ sort: false }).hash
export const hashObject = hasher({}).hash
