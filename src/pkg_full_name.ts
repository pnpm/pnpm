export const delimiter = '+'

export default pkg => pkg.name.replace('/', delimiter) + '@' + escapeVersion(pkg.version)

function escapeVersion (version) {
  if (!version) return ''
  return version.replace(/[/\\:]/g, delimiter)
}
