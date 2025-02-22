export function shouldSyncInjectedDepsAfterScripts (scriptName: string, syncInjectedDepsAfterScripts: boolean | string[] | undefined): boolean {
  return typeof syncInjectedDepsAfterScripts === 'boolean'
    ? syncInjectedDepsAfterScripts
    : syncInjectedDepsAfterScripts?.includes(scriptName) ?? false
}
