module.exports = jest.createMockFromModule('@pnpm/fetch')

// default implementation
module.exports.fetch.mockImplementation(async (_url, _opts) => {
  return { ok: true }
})
