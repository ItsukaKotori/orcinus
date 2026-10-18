import { expect, it } from 'vitest'
import { gitCommitFilesSchema, gitHistoryResultSchema } from './types'
import { buildFixtureSnapshot } from '../fixtures/fixture-snapshot'
import { FixtureRepository } from '../fixtures/fixture-repository'

// The fixture is authored to mirror the host `ade_git::history` serde (camelCase)
// shape, and HostRepository validates host responses against these same schemas.
// So a fixture that parses ⇒ the TS contract and the host shape agree.
it('fixture snapshot satisfies gitHistoryResultSchema (TS↔host serde contract)', () => {
  expect(() => gitHistoryResultSchema.parse(buildFixtureSnapshot())).not.toThrow()
})

it('fixture commit files satisfy gitCommitFilesSchema', async () => {
  const files = await new FixtureRepository().loadCommitFiles('a'.repeat(40))
  expect(() => gitCommitFilesSchema.parse(files)).not.toThrow()
})
