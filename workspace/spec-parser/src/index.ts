const WORKSPACE_PREF_REGEX = /^workspace:((?<alias>[^._/][^@]*)@)?(?<version>.*)$/

export interface ParsedWorkspacePref {
  alias?: string
  version: string
}

export function parseWorkspacePref (pref: string): ParsedWorkspacePref | null {
  const parts = WORKSPACE_PREF_REGEX.exec(pref)
  if (parts === null) return null
  return parts.groups! as unknown as ParsedWorkspacePref
}
