export function getNodeMirror (nodeDownloadMirrors: Record<string, string> | undefined, releaseChannel: string): string {
  const nodeMirror = nodeDownloadMirrors?.[releaseChannel] ?? `https://nodejs.org/download/${releaseChannel}/`
  return normalizeNodeMirror(nodeMirror)
}

function normalizeNodeMirror (nodeMirror: string): string {
  return nodeMirror.endsWith('/') ? nodeMirror : `${nodeMirror}/`
}
