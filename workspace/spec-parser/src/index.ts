const WORKSPACE_PREF_REGEX = /^workspace:(?:(?<alias>[^._/][^@]*)@)?(?<version>.*)$/

export class WorkspaceSpec {
  alias?: string
  version: string

  constructor (version: string, alias?: string) {
    this.version = version
    this.alias = alias
  }

  static parse (pref: string): WorkspaceSpec | null {
    const parts = WORKSPACE_PREF_REGEX.exec(pref)
    if (!parts?.groups) return null
    return new WorkspaceSpec(parts.groups.version, parts.groups.alias)
  }

  toString (): `workspace:${string}` {
    const { alias, version } = this
    return alias ? `workspace:${alias}@${version}` : `workspace:${version}`
  }
}
