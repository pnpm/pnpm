import { parseWorkspacePref } from '@pnpm/workspace.spec-parser'

export function workspacePrefToNpm (workspacePref: string): string {
  const parseResult = parseWorkspacePref(workspacePref)
  if (parseResult == null) {
    throw new Error(`Invalid workspace spec: ${workspacePref}`)
  }

  const { alias, version } = parseResult
  const versionPart = version === '^' || version === '~' ? '*' : version
  return alias
    ? `npm:${alias}@${versionPart}`
    : versionPart
}
