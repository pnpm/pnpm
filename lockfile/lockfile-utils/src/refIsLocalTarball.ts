export function refIsLocalTarball (ref: string): boolean {
  return ref.startsWith('file:') && (ref.endsWith('.tgz') || ref.endsWith('.tar.gz') || ref.endsWith('.tar'))
}

export function refIsLocalDirectory (ref: string): boolean {
  return ref.startsWith('file:') && !refIsLocalTarball(ref)
}
