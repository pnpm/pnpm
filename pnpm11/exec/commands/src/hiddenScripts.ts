import { PnpmError } from '@pnpm/error'

/**
 * Filters out hidden scripts (starting with '.') when called outside a lifecycle.
 * Throws if the user explicitly requested a hidden script by exact name,
 * or if all matched scripts are hidden.
 */
export function throwOrFilterHiddenScripts (specifiedScripts: string[], scriptName: string): string[] {
  if (specifiedScripts.length === 0) return specifiedScripts
  const hidden = specifiedScripts.filter((s) => s.startsWith('.'))
  if (hidden.length === 0) return specifiedScripts
  // Exact name request for a hidden script
  if (scriptName.startsWith('.')) {
    throw new PnpmError('HIDDEN_SCRIPT', `Script "${scriptName}" is hidden and cannot be run directly`, {
      hint: 'Scripts starting with "." are hidden and can only be called from other scripts.',
    })
  }
  // Regex/glob matched both visible and hidden — filter out hidden
  const visible = specifiedScripts.filter((s) => !s.startsWith('.'))
  if (visible.length > 0) return visible
  // Only hidden scripts matched
  throw new PnpmError('HIDDEN_SCRIPT', `All matched scripts are hidden and cannot be run directly: ${hidden.join(', ')}`, {
    hint: 'Scripts starting with "." are hidden and can only be called from other scripts.',
  })
}
