import type { Completion } from '@pnpm/types'

export function optionTypesToCompletions(optionTypes: Record<string, unknown>): Completion[] {
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
