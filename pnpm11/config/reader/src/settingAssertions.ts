import { PnpmError } from '@pnpm/error'

export function renderReceivedType (value: unknown): string {
  if (value === null) return 'null'
  if (Array.isArray(value)) return 'array'
  return typeof value
}

// The `update`, `audit` and `packageExtensions` sections come from repo-controlled
// pnpm-workspace.yaml, which is parsed untyped — so their fields are validated
// here. An invalid `audit.level` is especially worth catching: it would leave
// `pnpm audit` comparing severities against `undefined`, silently reporting no
// advisories.
export function assertStringArray (value: unknown, settingName: string): asserts value is string[] {
  if (!Array.isArray(value) || value.some((item) => typeof item !== 'string')) {
    throw new PnpmError('INVALID_SETTING', `The "${settingName}" setting should be an array of strings, but got ${renderReceivedType(value)}`)
  }
}

export function assertOptionalBoolean (value: unknown, settingName: string): void {
  if (value == null) return
  assertBoolean(value, settingName)
}

export function assertBoolean (value: unknown, settingName: string): asserts value is boolean {
  if (typeof value !== 'boolean') {
    throw new PnpmError('INVALID_SETTING', `The "${settingName}" setting should be a boolean, but got ${renderReceivedType(value)}`)
  }
}

export function assertString (value: unknown, settingName: string): asserts value is string {
  if (typeof value !== 'string') {
    throw new PnpmError('INVALID_SETTING', `The "${settingName}" setting should be a string, but got ${renderReceivedType(value)}`)
  }
}

export function assertStringRecord (value: unknown, settingName: string): void {
  assertObjectSetting(value, settingName)
  for (const [key, item] of Object.entries(value as Record<string, unknown>)) {
    assertString(item, `${settingName}.${key}`)
  }
}

// Not an `asserts` guard on purpose: it only rejects malformed shapes at
// runtime, without narrowing away the section's declared type at the call site.
export function assertObjectSetting (value: unknown, settingName: string): void {
  if (value == null || typeof value !== 'object' || Array.isArray(value)) {
    throw new PnpmError('INVALID_SETTING', `The "${settingName}" setting should be an object, but got ${renderReceivedType(value)}`)
  }
}

