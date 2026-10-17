import { describe, expect, it } from 'vitest'
import { QUICK_OPEN_QUERY_MAX_BYTES } from '../quick-open-search'
import {
  classifyTabEntryQuery,
  getTabEntryOptions,
  TAB_ENTRY_ABSOLUTE_PATH_REMOTE_BLOCKED_MESSAGE,
  validateNewTabEntryAbsolutePath,
  validateNewTabEntryRelativePath
} from './tab-create-entry-action'

const readyFiles = (files: string[]) => ({ files, loading: false, loadError: null })

describe('tab create entry classification', () => {
  // Kept word-for-word in step with the omnibox placeholder (see
  // TabBarCreateEntry.keyboard.test.tsx), so the two never drift apart.
  it('advertises the omnibox placeholder in the empty-query message', () => {
    expect(classifyTabEntryQuery('', readyFiles([]))).toEqual({
      kind: 'empty',
      message: 'Search open tabs, files, agents…'
    })
  })

  it('does not classify invalid numeric hosts as URLs', () => {
    expect(classifyTabEntryQuery('999.999.999.999', readyFiles([]))).toEqual({
      kind: 'new-file',
      relativePath: '999.999.999.999'
    })
  })

  it('keeps common source/document filenames as file candidates', () => {
    expect(classifyTabEntryQuery('README.md', readyFiles([]))).toEqual({
      kind: 'new-file',
      relativePath: 'README.md'
    })
    expect(classifyTabEntryQuery('src/foo.test.ts', readyFiles([]))).toEqual({
      kind: 'new-file',
      relativePath: 'src/foo.test.ts'
    })
    expect(classifyTabEntryQuery('docs/readme.md', readyFiles([]))).toEqual({
      kind: 'new-file',
      relativePath: 'docs/readme.md'
    })
  })

  it('matches exact relative path before basename and fuzzy results', () => {
    const files = readyFiles(['src/index.ts', 'docs/index.ts', 'src/components/Button.tsx'])
    expect(classifyTabEntryQuery('docs/index.ts', files)).toEqual({
      kind: 'existing-file',
      matchKind: 'exact-path',
      relativePath: 'docs/index.ts'
    })
    expect(classifyTabEntryQuery('Button.tsx', files)).toEqual({
      kind: 'existing-file',
      matchKind: 'exact-basename',
      relativePath: 'src/components/Button.tsx'
    })
    expect(classifyTabEntryQuery('btn', files)).toEqual({
      kind: 'existing-file',
      matchKind: 'fuzzy',
      relativePath: 'src/components/Button.tsx'
    })
  })

  // Fuzzy matching is a subsequence scan, so a short token matches broadly.
  it('keeps file matches for path prefixes that cannot be created', () => {
    const files = ['src/main/index.ts', 'src/renderer/App.tsx']
    // A trailing separator is an ordinary keystroke on the way to a nested path,
    // so the matches it finds must survive the unusable-path verdict.
    for (const query of ['src/', 'src/renderer/']) {
      expect(
        getTabEntryOptions(query, readyFiles(files)).map((option) => option.classification.kind),
        query
      ).toEqual(expect.arrayContaining(['existing-file']))
    }
    expect(getTabEntryOptions('src/', readyFiles([]))).toMatchObject([
      { classification: { kind: 'blocked' } }
    ])
  })

  it('reports the running scan for path-shaped text it cannot create yet', () => {
    expect(
      getTabEntryOptions('src/', { files: [], loading: true, loadError: null }).map(
        (option) => option.id
      )
    ).toEqual(['loading'])
  })

  it('never turns invalid paths into search actions', () => {
    for (const query of ['../.env', 'foo//bar', 'foo/', 'C:relative.txt', 'src/\u0000file.ts']) {
      expect(classifyTabEntryQuery(query, readyFiles([])), query).toMatchObject({ kind: 'blocked' })
      expect(
        classifyTabEntryQuery(query, { files: [], loading: true, loadError: null }),
        query
      ).toMatchObject({ kind: 'blocked' })
    }
  })

  it('blocks oversized pasted file-entry queries before reading listed files', () => {
    const oversizedQuery = `src/${'secret-tab-create'.repeat(QUICK_OPEN_QUERY_MAX_BYTES)}.ts`
    const fileList = {
      get files(): string[] {
        throw new Error('oversized queries must not read file lists')
      },
      loading: false,
      loadError: null
    }

    expect(classifyTabEntryQuery(oversizedQuery, fileList)).toEqual({
      kind: 'blocked',
      message: 'Search text is too large.'
    })
  })

  it('blocks oversized whitespace before trimming file-entry queries', () => {
    const fileList = {
      get files(): string[] {
        throw new Error('oversized whitespace queries must not read file lists')
      },
      loading: false,
      loadError: null
    }

    expect(classifyTabEntryQuery(' '.repeat(QUICK_OPEN_QUERY_MAX_BYTES + 1), fileList)).toEqual({
      kind: 'blocked',
      message: 'Search text is too large.'
    })
  })

  it('classifies POSIX absolute paths only for POSIX clients', () => {
    expect(
      classifyTabEntryQuery('/tmp/notes.md', readyFiles([]), {
        allowAbsolutePaths: true,
        localPlatform: 'posix'
      })
    ).toEqual({ kind: 'absolute-file', filePath: '/tmp/notes.md' })
    expect(
      classifyTabEntryQuery('C:\\tmp\\notes.md', readyFiles([]), {
        allowAbsolutePaths: true,
        localPlatform: 'posix'
      })
    ).toMatchObject({ kind: 'blocked', message: 'Enter an absolute path for this computer.' })
  })

  it('classifies drive and UNC paths only for Windows clients', () => {
    for (const [query, filePath] of [
      ['C:\\tmp\\notes.md', 'C:/tmp/notes.md'],
      ['C:/tmp/notes.md', 'C:/tmp/notes.md'],
      ['\\\\server\\share\\notes.md', '//server/share/notes.md'],
      ['//server/share/notes.md', '//server/share/notes.md']
    ]) {
      expect(
        classifyTabEntryQuery(query, readyFiles([]), {
          allowAbsolutePaths: true,
          localPlatform: 'windows'
        })
      ).toEqual({
        kind: 'absolute-file',
        filePath
      })
    }
    expect(
      classifyTabEntryQuery('/tmp/notes.md', readyFiles([]), {
        allowAbsolutePaths: true,
        localPlatform: 'windows'
      })
    ).toMatchObject({ kind: 'blocked', message: 'Enter an absolute path for this computer.' })
  })

  it('blocks absolute paths for remote workspaces', () => {
    for (const query of [
      '/tmp/notes.md',
      'C:\\tmp\\notes.md',
      'C:/tmp/notes.md',
      '\\\\server\\share\\notes.md',
      '//server/share/notes.md'
    ]) {
      expect(classifyTabEntryQuery(query, readyFiles([]))).toMatchObject({
        kind: 'blocked',
        message: TAB_ENTRY_ABSOLUTE_PATH_REMOTE_BLOCKED_MESSAGE
      })
    }
  })
})

describe('tab create entry path validation', () => {
  it('rejects unsafe or non-relative paths', () => {
    for (const path of [
      '',
      '/tmp/file.ts',
      'C:/tmp/file.ts',
      'C:tmp/file.ts',
      '\\\\server\\share\\file.ts',
      '~',
      '~/file.ts',
      'src/',
      'src//file.ts',
      'src/../file.ts',
      'src\\.\\file.ts',
      'src\\..\\file.ts',
      'src/\u0000file.ts'
    ]) {
      expect(() => validateNewTabEntryRelativePath(path), path).toThrow()
    }
  })

  it('allows spaces and normalizes Windows separators after absolute checks', () => {
    expect(validateNewTabEntryRelativePath(' docs/My Note.md ')).toBe('docs/My Note.md')
    expect(validateNewTabEntryRelativePath('src\\new-file.ts')).toBe('src/new-file.ts')
  })

  it('normalizes absolute paths for local open', () => {
    expect(validateNewTabEntryAbsolutePath('/tmp/notes.md', 'posix')).toBe('/tmp/notes.md')
    expect(validateNewTabEntryAbsolutePath('C:\\tmp\\notes.md', 'windows')).toBe('C:/tmp/notes.md')
    expect(validateNewTabEntryAbsolutePath('\\\\server\\share\\notes.md', 'windows')).toBe(
      '//server/share/notes.md'
    )
    expect(validateNewTabEntryAbsolutePath('/repo/../repo/src/file.ts', 'posix')).toBe(
      '/repo/src/file.ts'
    )
  })

  it('rejects path families that are not native to the client', () => {
    expect(() => validateNewTabEntryAbsolutePath('C:\\tmp\\notes.md', 'posix')).toThrow(
      'this computer'
    )
    expect(() => validateNewTabEntryAbsolutePath('\\\\server\\share\\notes.md', 'posix')).toThrow(
      'this computer'
    )
    expect(() => validateNewTabEntryAbsolutePath('/tmp/notes.md', 'windows')).toThrow(
      'this computer'
    )
  })

  it('rejects invalid absolute paths', () => {
    for (const path of ['', '~/file.ts', 'src/file.ts', 'C:tmp/file.ts', '/tmp/']) {
      expect(() => validateNewTabEntryAbsolutePath(path), path).toThrow()
    }
  })
})
