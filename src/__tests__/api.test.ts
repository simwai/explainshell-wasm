import { describe, expect, it, vi } from 'vitest'
import type { ExplainOptions, ExplainResult } from '../types.js'

vi.mock('../runtime/node.js', () => ({
  createExplainshell: vi.fn(),
  resetCache: vi.fn(),
}))

vi.mock('../runtime/browser.js', () => ({
  createExplainshell: vi.fn(),
  resetCache: vi.fn(),
}))

describe('Explainshell API types', () => {
  it('should have correct ExplainOptions structure', () => {
    const options: ExplainOptions = {
      distro: 'ubuntu',
      release: '26.04',
    }

    expect(options.distro).toBe('ubuntu')
    expect(options.release).toBe('26.04')
  })

  it('should have correct ExplainResult structure', () => {
    const result: ExplainResult = {
      groups: [
        {
          name: 'shell',
          results: [],
          positional_index: 0,
        },
        {
          name: 'command1',
          results: [
            {
              start: 0,
              end: 3,
              text: 'manipulate tape archives',
              match_text: 'tar',
              debug_info: { kind: 'synopsis' },
            },
          ],
          manpage: {
            source: 'ubuntu/26.04/1/tar.1.gz',
            name: 'tar',
            synopsis: 'manipulate tape archives',
            options: [],
            aliases: [['tar', 10]],
            dashless_opts: false,
            subcommands: [],
            updated: false,
            nested_cmd: false,
          },
          positional_index: 0,
        },
      ],
      expansions: [],
    }

    expect(result.groups).toHaveLength(2)
    expect(result.groups[1]?.manpage?.name).toBe('tar')
    expect(result.groups[1]?.results[0]?.text).toBe('manipulate tape archives')
  })

  it('should allow unknown matches with null text', () => {
    const result: ExplainResult = {
      groups: [
        {
          name: 'command1',
          results: [{ start: 0, end: 9, text: null, match_text: null }],
          positional_index: 0,
        },
      ],
      expansions: [],
    }

    expect(result.groups[0]?.results[0]?.text).toBeNull()
  })
})
