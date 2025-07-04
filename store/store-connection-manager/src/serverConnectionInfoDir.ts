import path from 'node:path'

export function serverConnectionInfoDir (storePath: string): string {
  return path.join(storePath, 'server')
}
