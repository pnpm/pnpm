export interface ReportIdentity {
  /** The manifest's `name`, when it declares one. */
  name?: string
  /** The project's directory, relative to the workspace root. */
  dirKey: string
}

/**
 * Chooses a distinct `pnpm -r pkg get` report key for every project, in input order.
 *
 * A project is keyed by its name unless that would hide another project:
 * a project without a name, every project that shares its name with
 * another selected project, and a project whose name equals the directory
 * key another project is reported under are keyed by their directory instead. Directories
 * are distinct, so the keys are too, and a workspace whose names are unique
 * keeps its name keys.
 */
export function assignReportKeys (identities: ReportIdentity[]): string[] {
  const keyedByDir = initiallyKeyedByDir(identities)
  let moved: boolean
  do {
    moved = keyNamesShadowedByDirs(identities, keyedByDir)
  } while (moved)
  return identities.map((identity, index) =>
    identity.name == null || keyedByDir[index] ? identity.dirKey : identity.name
  )
}

function initiallyKeyedByDir (identities: ReportIdentity[]): boolean[] {
  const nameCounts = new Map<string, number>()
  for (const { name } of identities) {
    if (name != null) nameCounts.set(name, (nameCounts.get(name) ?? 0) + 1)
  }
  return identities.map(({ name }) => name == null || nameCounts.get(name)! > 1)
}

/**
 * Moves every name-keyed project whose name equals a directory key in use
 * to its own directory key. Returns whether any project moved, since a
 * moved project's directory key can in turn shadow another name.
 */
function keyNamesShadowedByDirs (identities: ReportIdentity[], keyedByDir: boolean[]): boolean {
  const dirKeys = new Set(identities.filter((_, index) => keyedByDir[index]).map(({ dirKey }) => dirKey))
  let moved = false
  for (const [index, { name }] of identities.entries()) {
    if (!keyedByDir[index] && name != null && dirKeys.has(name)) {
      keyedByDir[index] = true
      moved = true
    }
  }
  return moved
}
