import path from 'path'
import exists from 'path-exists'

export default async (entityName: string, dir: string) => {
  const entityPath = path.join(dir, entityName)
  if (await exists(entityPath)) return entityPath
  return undefined
}
