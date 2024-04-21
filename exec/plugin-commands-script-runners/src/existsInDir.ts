import path from 'path'
import exists from 'path-exists'

export async function existsInDir (entityName: string, dir: string): Promise<string | undefined> {
  const entityPath = path.join(dir, entityName)
  if (await exists(entityPath)) return entityPath
  return undefined
}
