import { type JsrSpec, type JsrSpecWithAlias, type ParsedJsrPackageName } from './types'

export function createJsrPref (parsed: JsrSpec): string {
  if (parsed.scope == null) {
    return `jsr:${parsed.spec}`
  }

  let pref = `jsr:${createJsrPackageName(parsed)}`
  if (parsed.spec) {
    pref += `@${parsed.spec}`
  }
  return pref
}

export function createJsrPackageName ({ scope, name }: ParsedJsrPackageName): string {
  return `@${scope}/${name}`
}

export function createNpmPref (jsr: JsrSpecWithAlias): string {
  let pref = `npm:${createNpmPackageName(jsr)}`
  if (jsr.spec) {
    pref += `@${jsr.spec}`
  }
  return pref
}

export function createNpmPackageName ({ scope, name }: ParsedJsrPackageName): string {
  return `@jsr/${scope}__${name}`
}
