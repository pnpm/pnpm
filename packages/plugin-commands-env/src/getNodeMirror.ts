import { Config } from '@pnpm/config'

export default function getNodeMirror (rawConfig: Config['rawConfig'], releaseDir: string): string {
  // This is a dynamic lookup since the 'use-node-version' option is allowed to be '<releaseDir>/<version>'
  const configKey = `node-mirror:${releaseDir}`
  const nodeMirror = rawConfig[configKey] ?? `https://nodejs.org/download/${releaseDir}/`
  return normalizeNodeMirror(nodeMirror)
}

function normalizeNodeMirror (nodeMirror: string): string {
  return nodeMirror.endsWith('/') ? nodeMirror : `${nodeMirror}/`
}
