import replaceString = require('replace-string')

// The only reason package IDs are encoded is to avoid '>' signs.
// Otherwise, it would be impossible to split the node ID back to package IDs reliably.
// See issue https://github.com/pnpm/pnpm/issues/986
export default function encodePkgId (pkgId: string) {
  return replaceString(replaceString(pkgId, '%', '%25'), '>', '%3E')
}
