import path from 'node:path'

/**
 * Where the hoisted linker nests the dependencies of the workspace project at
 * `projectDir` that it cannot hoist to the root. `lockfileToHoistedDepGraph`
 * always uses `node_modules` here, whatever `modulesDir` says.
 */
export function getHoistedProjectModulesDir (projectDir: string): string {
  return path.join(projectDir, 'node_modules')
}
