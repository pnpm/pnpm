import { validRange } from 'semver'

export function isValidPeerRange (version: string): boolean {
  // we use `includes` instead of `startsWith` because `workspace:*` and `catalog:*` could be a part of a wider version range expression
  return typeof validRange(version) === 'string' || version.includes('workspace:') || version.includes('catalog:')
}
