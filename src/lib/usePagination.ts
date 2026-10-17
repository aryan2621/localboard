import { useEffect, useMemo, useState } from 'react'

export const PAGE_SIZES = [10, 25, 50, 100] as const

export type Pagination<T> = {
  /** 1-based. */
  page: number
  pageSize: number
  pageCount: number
  total: number
  /** The slice of `items` on the current page. */
  pageItems: T[]
  setPage: (page: number) => void
  setPageSize: (size: number) => void
}

/**
 * Client-side paging over a list that is already loaded.
 * Pass `resetKey` (a filter, sort or search value) to jump back to page 1 whenever it changes.
 */
export function usePagination<T>(items: T[], options: { initialPageSize?: number; resetKey?: unknown } = {}): Pagination<T> {
  const { initialPageSize = 25, resetKey } = options
  const [page, setPageState] = useState(1)
  const [pageSize, setPageSizeState] = useState<number>(initialPageSize)
  const total = items.length
  const pageCount = Math.max(1, Math.ceil(total / pageSize))
  const current = Math.min(page, pageCount)

  useEffect(() => {
    setPageState(1)
  }, [resetKey])

  const pageItems = useMemo(() => items.slice((current - 1) * pageSize, current * pageSize), [items, current, pageSize])

  return {
    page: current,
    pageSize,
    pageCount,
    total,
    pageItems,
    setPage: (next) => setPageState(Math.min(Math.max(1, Math.trunc(next) || 1), pageCount)),
    setPageSize: (size) => {
      setPageSizeState(size)
      setPageState(1)
    },
  }
}
