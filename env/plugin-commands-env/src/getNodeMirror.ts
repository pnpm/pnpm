import { Config } from '@pnpm/config'

export function getNodeMirror (rawConfig: Config['rawConfig'], releaseChannel: string): string {
  // This is a dynamic lookup since the 'use-node-version' option is allowed to be '<releaseChannel>/<version>'
  const configKey = `node-mirror:${releaseChannel}`
  const nodeMirror = rawConfig[configKey] ?? `https://nodejs.org/download/${releaseChannel}/`
  return normalizeNodeMirror(nodeMirror)
}

function normalizeNodeMirror (nodeMirror: string): string {
  return nodeMirror.endsWith('/') ? nodeMirror : `${nodeMirror}/`
}
