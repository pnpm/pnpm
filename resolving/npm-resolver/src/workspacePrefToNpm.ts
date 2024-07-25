import { WorkspaceSpec } from '@pnpm/workspace.spec-parser'

export function workspacePrefToNpm (workspacePref: string): string {
  const parseResult = WorkspaceSpec.parse(workspacePref)
  if (parseResult == null) {
    throw new Error(`Invalid workspace spec: ${workspacePref}`)
  }

  const { alias, version } = parseResult
  const versionPart = version === '^' || version === '~' ? '*' : version
  return alias
    ? `npm:${alias}@${versionPart}`
    : versionPart
}
