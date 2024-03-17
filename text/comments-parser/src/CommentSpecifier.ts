export interface CommentSpecifier {
  type: string
  content: string
  lineNumber: number
  after?: string | undefined
  on: string
  whitespace: string
  before?: string | undefined
}
