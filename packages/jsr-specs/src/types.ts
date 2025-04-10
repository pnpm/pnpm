export interface JsrSpecBase {
  scope?: string
  name?: string
  spec?: string
}

/** Syntax: `jsr:<spec>` */
export interface JsrSpecWithoutAlias extends JsrSpecBase {
  scope?: undefined
  name?: undefined
  spec: string
}

/** Syntax: `jsr:@<scope>/<name>[@<spec>] */
export interface JsrSpecWithAlias extends JsrSpecBase {
  scope: string
  name: string
  spec?: string
}

export type JsrSpec = JsrSpecWithoutAlias | JsrSpecWithAlias

export interface ParsedJsrPackageName {
  scope: string
  name: string
}
