import fs from 'fs'
import path from 'path'

export function existsInDir (entityName: string, dir: string): string | undefined {
  const entityPath = path.join(dir, entityName)
  if (fs.existsSync(entityPath)) return entityPath
  return undefined
}
