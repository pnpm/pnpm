import { type JsrSpecWithAlias, type ParsedJsrPackageName } from './types'

export function createJsrPackageName ({ scope, name }: ParsedJsrPackageName): string {
  return `@${scope}/${name}`
}

export function createNpmPref (jsr: JsrSpecWithAlias): string {
  let pref = `npm:${createNpmPackageName(jsr)}`
  if (jsr.pref) {
    pref += `@${jsr.pref}`
  }
  return pref
}

export function createNpmPackageName ({ scope, name }: ParsedJsrPackageName): string {
  return `@jsr/${scope}__${name}`
}
