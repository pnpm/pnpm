/**
 * How to take back what `pnpm setup` just did under `pnpmHome`, printed once
 * it succeeds. `pnpm setup` names the shell config file it changed and prints
 * the lines it added, so the hint points back at that output rather than
 * guessing the file again.
 */
export function renderUndoHint (pnpmHome: string, platform: NodeJS.Platform = process.platform): string {
  const undo = platform === 'win32'
    ? `delete ${pnpmHome}, then remove the PNPM_HOME variable and the Path entry pnpm setup added from your user environment variables.`
    : `delete ${pnpmHome} and the lines pnpm setup added to the shell config file named above.`
  return `To uninstall, ${undo}\nSee https://pnpm.io/uninstall`
}
