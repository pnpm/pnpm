import path from 'path'
import { spawnSync } from 'child_process'
import camelcaseKeys from 'camelcase-keys'

interface Person {
  name?: string
  email?: string
  url?: string
  web?: string
  mail?: string
}

export function unParsePerson (person: string | Person): string {
  if (typeof person === 'string') {
    return person
  }
  const name = person.name ?? ''
  const u = person.url ?? person.web
  const url = u ? (' (' + u + ')') : ''
  const e = person.email ?? person.mail
  const email = e ? (' <' + e + '>') : ''
  return name + email + url
}

function workWithInitModule (localConfig: Record<string, string>) {
  if ('initModule' in localConfig && localConfig.initModule) {
    const filePath = path.resolve(localConfig.initModule)
    spawnSync('node', [filePath], {
      stdio: 'inherit',
    })
    delete localConfig.initModule
  }
  return localConfig
}

function workWithIncludeWorkspaceRoot (localConfig: Record<string, string>) {
  if ('includeWorkspaceRoot' in localConfig) {
    // TODO
    delete localConfig.includeWorkspaceRoot
  }
  return localConfig
}

function workWithInitConfig (localConfig: Record<string, string>) {
  const packageJson: Record<string, string> = {}
  const authorInfo: Record<string, string> = {}
  for (const localConfigKey in localConfig) {
    const value = localConfig[localConfigKey]
    const isInitKey = localConfigKey.startsWith('init')
    if (isInitKey) {
      const key = localConfigKey.replace('init', '')
      const isAuthorKey = key.startsWith('Author')
      if (isAuthorKey) {
        authorInfo[key.replace('Author', '')] = value
      } else {
        packageJson[key] = value
      }
    }
  }

  const author = unParsePerson(camelcaseKeys(authorInfo))
  if (author) packageJson.author = author
  return camelcaseKeys(packageJson)
}

export async function parseRawConfig (rawConfig: Record<string, string>): Promise<Record<string, string>> {
  return workWithInitConfig(
    workWithIncludeWorkspaceRoot(
      workWithInitModule(camelcaseKeys(rawConfig))
    )
  )
}
