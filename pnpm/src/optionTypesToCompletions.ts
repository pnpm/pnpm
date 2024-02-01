import { type CompletionItem } from '@pnpm/tabtab'

export function optionTypesToCompletions (optionTypes: Record<string, unknown>) {
  const completions: CompletionItem[] = []
  for (const [name, typeObj] of Object.entries(optionTypes)) {
    if (typeObj === Boolean) {
      completions.push({ name: `--${name}` })
      completions.push({ name: `--no-${name}` })
    } else {
      completions.push({ name: `--${name}` })
    }
  }
  return completions
}
