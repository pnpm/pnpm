import path from 'path'

export function serverConnectionInfoDir (storePath: string) {
  return path.join(storePath, 'server')
}
