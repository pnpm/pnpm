module.exports = jest.createMockFromModule('@pnpm/network.fetch')

// default implementation
module.exports.fetchWithAgent.mockImplementation(async (_url, _opts) => {
  return { ok: true }
})
