import type { MarkdownTokenizer } from '@tiptap/core'
import { OrderedList } from '@tiptap/extension-list'

const baseTokenizer = OrderedList.config.markdownTokenizer as MarkdownTokenizer

// Why: tiptap 3.31 stubs the ordered-list tokenizer's `start` to () => -1, so the guard cannot
// use it. Match the base's first-line marker shape (`ORDERED_LIST_ITEM_REGEX`) cheaply instead.
const ORDERED_LIST_START = /^\s*(?:\d+|[ivxlcdmIVXLCDM]+|[a-zA-Z]{1,2})[.)]\s+/

export const RichMarkdownOrderedList = OrderedList.extend({
  markdownTokenizer: {
    ...baseTokenizer,
    tokenize(src, tokens, lexer) {
      // Why: the base tokenizer scans the full remaining source before rejecting a non-list.
      if (!ORDERED_LIST_START.test(src)) {
        return undefined
      }
      return baseTokenizer.tokenize(src, tokens, lexer)
    }
  }
})
