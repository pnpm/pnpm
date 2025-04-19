import { type JsrSpecWithAlias, type ParsedJsrPackageName } from './types'

export function jsrToNpmSpecifier (jsr: JsrSpecWithAlias): string {
  let npmSpecifier = `npm:${jsrToNpmPackageName(jsr)}`
  if (jsr.pref) {
    npmSpecifier += `@${jsr.pref}`
  }
  return npmSpecifier
}

export function jsrToNpmPackageName ({ scope, name }: ParsedJsrPackageName): string {
  return `@jsr/${scope}__${name}`
}
