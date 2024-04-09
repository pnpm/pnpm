// The only reason package IDs are encoded is to avoid '>' signs.
// Otherwise, it would be impossible to split the node ID back to package IDs reliably.
// See issue https://github.com/pnpm/pnpm/issues/986
export function encodePkgId (pkgId: string): string {
  return pkgId.replaceAll('%', '%25').replaceAll('>', '%3E')
}
