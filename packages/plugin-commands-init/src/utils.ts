import path from 'path'
import { spawnSync } from 'child_process'
import camelcaseKeys from 'camelcase-keys'
import fs from 'fs'

export interface Person {
  name?: string
  email?: string
  url?: string
  web?: string
  mail?: string
}

export function personToString (person: Person): string {
  const name = person.name ?? ''
  const u = person.url ?? person.web
  const url = u ? ` (${u})` : ''
  const e = person.email ?? person.mail
  const email = e ? ` <${e}>` : ''
  return name + email + url
}

export function workWithInitModule (localConfig: Record<string, string>) {
  const { initModule, ...restConfig } = localConfig
  if (initModule) {
    const filePath = path.resolve(localConfig.initModule)
    const isFileExist = fs.existsSync(filePath)
    if (['.js', '.cjs'].includes(path.extname(filePath)) && isFileExist) {
      spawnSync('node', [filePath], {
        stdio: 'inherit',
      })
    }
  }
  return restConfig
}

export function workWithInitConfig (localConfig: Record<string, string>) {
  const packageJson: Record<string, string> = {}
  const authorInfo: Record<string, string> = {}
  for (const localConfigKey in localConfig) {
    if (localConfigKey.startsWith('init')) {
      const pureKey = localConfigKey.replace('init', '')
      const value = localConfig[localConfigKey]
      if (pureKey.startsWith('Author')) {
        authorInfo[pureKey.replace('Author', '')] = value
      } else {
        packageJson[pureKey] = value
      }
    }
  }

  const author = personToString(camelcaseKeys(authorInfo))
  if (author) {
    packageJson.author = author
  }
  return camelcaseKeys(packageJson)
}

export async function parseRawConfig (rawConfig: Record<string, string>): Promise<Record<string, string>> {
  return workWithInitConfig(
    workWithInitModule(camelcaseKeys(rawConfig))
  )
}
