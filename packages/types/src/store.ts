export interface StoreIndex {
  // package ID => paths of dependent projects
  [pkgId: string]: string[],
}
