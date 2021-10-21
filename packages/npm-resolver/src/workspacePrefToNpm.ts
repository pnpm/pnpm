export default function workspacePrefToNpm (workspacePref: string): string {
  const prefParts = /^workspace:([^._/][^@]*@)?(.*)$/.exec(workspacePref)

  if (prefParts == null) {
    throw new Error(`Invalid workspace spec: ${workspacePref}`)
  }
  const [workspacePkgAlias, workspaceVersion] = prefParts.slice(1)

  const pkgAliasPart = workspacePkgAlias != null && workspacePkgAlias
    ? `npm:${workspacePkgAlias}`
    : ''
  const versionPart = workspaceVersion === '^' || workspaceVersion === '~'
    ? '*'
    : workspaceVersion

  return `${pkgAliasPart}${versionPart}`
}
