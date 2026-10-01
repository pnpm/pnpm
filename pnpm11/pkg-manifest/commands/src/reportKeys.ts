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
  moveNamesShadowedByDirs(identities, keyedByDir)
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
 * to its own directory key. A moved project's directory key can in turn
 * shadow another name, so each move is followed up the same way.
 */
function moveNamesShadowedByDirs (identities: ReportIdentity[], keyedByDir: boolean[]): void {
  const nameKeyed = new Map<string, number>()
  for (const [index, { name }] of identities.entries()) {
    if (name != null && !keyedByDir[index]) nameKeyed.set(name, index)
  }
  const pending = identities.flatMap((_, index) => keyedByDir[index] ? [index] : [])
  for (let index = pending.pop(); index !== undefined; index = pending.pop()) {
    const shadowed = nameKeyed.get(identities[index].dirKey)
    if (shadowed !== undefined && !keyedByDir[shadowed]) {
      keyedByDir[shadowed] = true
      pending.push(shadowed)
    }
  }
}
