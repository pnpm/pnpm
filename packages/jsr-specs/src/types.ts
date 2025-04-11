export interface JsrSpecBase {
  scope?: string
  name?: string
  pref?: string
}

/** Syntax: `jsr:<spec>` */
export interface JsrSpecWithoutAlias extends JsrSpecBase {
  scope?: undefined
  name?: undefined
  pref: string
}

/** Syntax: `jsr:@<scope>/<name>[@<spec>] */
export interface JsrSpecWithAlias extends JsrSpecBase {
  scope: string
  name: string
  pref?: string
}

export type JsrSpec = JsrSpecWithoutAlias | JsrSpecWithAlias

export interface ParsedJsrPackageName {
  scope: string
  name: string
}
