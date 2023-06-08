export function refIsLocalTarball (ref: string) {
  return ref.startsWith('file:') && (ref.endsWith('.tgz') || ref.endsWith('.tar.gz') || ref.endsWith('.tar'))
}

export function refIsLocalDirectory (ref: string) {
  return ref.startsWith('file:') && !refIsLocalTarball(ref)
}
