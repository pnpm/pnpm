import {Package} from '../types'

export const delimiter = '+'

export default (pkg: Package): string => pkg.name.replace('/', delimiter) + '@' + escapeVersion(pkg.version)

function escapeVersion (version: string) {
  if (!version) return ''
  return version.replace(/[/\\:]/g, delimiter)
}
