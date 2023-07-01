import { promises as fs, readFileSync } from 'fs'

export function syncJSON<T> (path: string): T {
  const content = readFileSync(path, 'utf-8')
  return JSON.parse(content) as T
}

export async function asyncJSON<T> (path: string): Promise<T> {
  const content = await fs.readFile(path, 'utf-8')
  return JSON.parse(content) as T
}
