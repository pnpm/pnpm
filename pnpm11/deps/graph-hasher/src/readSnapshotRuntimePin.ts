function extractRuntimeNodeVersion (snapshotKey: string): string | undefined {
  const prefix = 'node@runtime:'
  if (!snapshotKey.startsWith(prefix)) return undefined
  const versionWithPeers = snapshotKey.slice(prefix.length)
  const parenAt = versionWithPeers.indexOf('(')
  return parenAt === -1 ? versionWithPeers : versionWithPeers.slice(0, parenAt)
}

export function readSnapshotRuntimePin (
  children: Record<string, string> | undefined
): string | undefined {
  const ref = children?.node
  return ref != null ? extractRuntimeNodeVersion(ref) : undefined
}
