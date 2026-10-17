import { useState } from 'react'

export type ViewMode = 'cards' | 'list'

/** Table (default) or cards for a page, remembered per page in this browser when storage is available. */
export function useViewMode(key: string, initial: ViewMode = 'list') {
  // v2: the table became the default, so earlier saved choices start over once.
  const storageKey = `view.v2.${key}`
  const [mode, setMode] = useState<ViewMode>(() => {
    try {
      const saved = localStorage.getItem(storageKey)
      return saved === 'list' || saved === 'cards' ? saved : initial
    } catch {
      return initial
    }
  })
  const update = (next: ViewMode) => {
    setMode(next)
    try {
      localStorage.setItem(storageKey, next)
    } catch {
      // Storage can be unavailable; the choice then lasts for this visit only.
    }
  }
  return [mode, update] as const
}

