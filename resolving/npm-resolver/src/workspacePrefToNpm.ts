import { WorkspaceSpec } from '@pnpm/workspace.spec-parser'

export function workspacePrefToNpm (workspaceBareSpecifier: string): string {
  const parseResult = WorkspaceSpec.parse(workspaceBareSpecifier)
  if (parseResult == null) {
    throw new Error(`Invalid workspace spec: ${workspaceBareSpecifier}`)
  }

  const { alias, version } = parseResult
  const versionPart = version === '^' || version === '~' ? '*' : version
  return alias
    ? `npm:${alias}@${versionPart}`
    : versionPart
}
