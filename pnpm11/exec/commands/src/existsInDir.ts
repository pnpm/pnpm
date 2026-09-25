import fs from 'node:fs'
import path from 'node:path'

export function existsInDir (entityName: string, dir: string): string | undefined {
  const entityPath = path.join(dir, entityName)
  if (fs.existsSync(entityPath)) return entityPath
  return undefined
}
