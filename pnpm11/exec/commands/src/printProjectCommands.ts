import type { ProjectManifest } from '@pnpm/types'

const ALL_LIFECYCLE_SCRIPTS = new Set([
  'prepublish',
  'prepare',
  'prepublishOnly',
  'prepack',
  'postpack',
  'publish',
  'postpublish',
  'preinstall',
  'install',
  'postinstall',
  'preuninstall',
  'uninstall',
  'postuninstall',
  'preversion',
  'version',
  'postversion',
  'pretest',
  'test',
  'posttest',
  'prestop',
  'stop',
  'poststop',
  'prestart',
  'start',
  'poststart',
  'prerestart',
  'restart',
  'postrestart',
  'preshrinkwrap',
  'shrinkwrap',
  'postshrinkwrap',
])

export function printProjectCommands (
  manifest: ProjectManifest,
  rootManifest?: ProjectManifest
): string {
  const { lifecycleScripts, otherScripts } = groupVisibleScripts(manifest)
  if (lifecycleScripts.length === 0 && otherScripts.length === 0) {
    return 'There are no scripts specified.'
  }
  const sections: string[] = []
  if (lifecycleScripts.length > 0) {
    sections.push(`Lifecycle scripts:\n${renderCommands(lifecycleScripts)}`)
  }
  if (otherScripts.length > 0) {
    sections.push(`Commands available via "pnpm run":\n${renderCommands(otherScripts)}`)
  }
  const rootScripts = Object.entries(rootManifest?.scripts ?? {})
  if (rootScripts.length > 0) {
    sections.push(`Commands of the root workspace project (to run them, use "pnpm -w run"):
${renderCommands(rootScripts)}`)
  }
  return sections.join('\n\n')
}

function groupVisibleScripts (manifest: ProjectManifest): { lifecycleScripts: string[][], otherScripts: string[][] } {
  const lifecycleScripts = [] as string[][]
  const otherScripts = [] as string[][]
  for (const [scriptName, script] of Object.entries(manifest.scripts ?? {})) {
    if (scriptName.startsWith('.')) continue
    if (ALL_LIFECYCLE_SCRIPTS.has(scriptName)) {
      lifecycleScripts.push([scriptName, script])
    } else {
      otherScripts.push([scriptName, script])
    }
  }
  return { lifecycleScripts, otherScripts }
}

function renderCommands (commands: string[][]): string {
  return commands.map(([scriptName, script]) => `  ${scriptName}\n    ${script}`).join('\n')
}
