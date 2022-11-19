import { Completion } from '@pnpm/command'

export function optionTypesToCompletions (optionTypes: Record<string, Object>) {
  const completions: Completion[] = []
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
