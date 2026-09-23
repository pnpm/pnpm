const LOCAL_TARBALL_EXTENSIONS = [
  '.tgz',
  '.tar.gz',
  '.tar',
  '.tar.bz2',
  '.tbz2',
  '.tbz',
]

export function refIsLocalTarball (ref: string): boolean {
  if (!ref.startsWith('file:')) return false
  const parenIdx = ref.indexOf('(')
  const cleanRef = parenIdx === -1 ? ref : ref.slice(0, parenIdx)
  const lower = cleanRef.toLowerCase()
  return LOCAL_TARBALL_EXTENSIONS.some((ext) => lower.endsWith(ext))
}

export function refIsLocalDirectory (ref: string): boolean {
  return ref.startsWith('file:') && !refIsLocalTarball(ref)
}
