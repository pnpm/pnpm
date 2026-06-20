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

export interface InitProperties {
  initAuthorName?: string
  initAuthorEmail?: string
  initAuthorUrl?: string
  initLicense?: string
  initVersion?: string
}

export function getInitConfig (opts: InitProperties): Record<string, string> {
  const packageJson: Record<string, string> = {}
  if (opts.initVersion) {
    packageJson.version = opts.initVersion
  }
  if (opts.initLicense) {
    packageJson.license = opts.initLicense
  }
  const author = personToString({
    name: opts.initAuthorName,
    email: opts.initAuthorEmail,
    url: opts.initAuthorUrl,
  })
  if (author) {
    packageJson.author = author
  }
  return packageJson
}
