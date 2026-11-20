import type { WantedDependency } from './getWantedDependencies.js'

export function wantedDepShouldUpdateCatalog (
  wantedDependency: Pick<WantedDependency, 'saveSpec' | 'updateSpec'> | undefined
): boolean {
  return wantedDependency?.updateSpec === true && wantedDependency.saveSpec !== false
}
