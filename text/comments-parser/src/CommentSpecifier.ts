export interface CommentSpecifier {
  type: string
  content: string
  lineNumber: number
  after?: string
  on: string
  whitespace: string
  before?: string
}
