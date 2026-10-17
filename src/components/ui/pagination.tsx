import { useEffect, useState } from 'react'
import { ChevronLeft, ChevronRight, ChevronsLeft, ChevronsRight } from 'lucide-react'
import { PAGE_SIZES } from '../../lib/usePagination'
import { cn } from '../../lib/utils'
import { Button } from './button'
import { Input } from './input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from './select'

type PaginationBarProps = {
  /** 1-based current page. */
  page: number
  pageSize: number
  /** Items known so far. With `hasMore`, more exist beyond this number. */
  total: number
  onPageChange: (page: number) => void
  onPageSizeChange: (size: number) => void
  /** True when more items can still be fetched (server paging), so the total is a lower bound. */
  hasMore?: boolean
  /** Shown only when there is more than one smallest page of items, unless this is set. */
  alwaysShow?: boolean
  pageSizeOptions?: readonly number[]
  className?: string
}

/** Page size, page number and first/previous/next/last controls for any list or table. */
export function PaginationBar({
  page,
  pageSize,
  total,
  onPageChange,
  onPageSizeChange,
  hasMore = false,
  alwaysShow = false,
  pageSizeOptions = PAGE_SIZES,
  className,
}: PaginationBarProps) {
  const pageCount = Math.max(1, Math.ceil(total / pageSize))
  // A page may start at a size that is not in the list (e.g. 12 for a 3-column grid); list it so it shows.
  const sizes = [...new Set([...pageSizeOptions, pageSize])].sort((a, b) => a - b)
  const [draft, setDraft] = useState(String(page))
  useEffect(() => setDraft(String(page)), [page])

  if (!alwaysShow && !hasMore && total <= Math.min(...pageSizeOptions)) return null

  const from = total === 0 ? 0 : (page - 1) * pageSize + 1
  const to = Math.min(total, page * pageSize)
  const atEnd = page >= pageCount
  const lastKnown = hasMore ? '+' : ''

  const commit = () => {
    const next = Number.parseInt(draft, 10)
    if (Number.isFinite(next)) onPageChange(Math.min(Math.max(1, next), pageCount))
    else setDraft(String(page))
  }

  return (
    <div className={cn('flex flex-wrap items-center justify-between gap-x-6 gap-y-2 text-app-caption text-muted-foreground', className)}>
      <p aria-live="polite">
        {from}–{to} of {total}
        {lastKnown}
      </p>
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
        <div className="flex items-center gap-2">
          <span id="rows-per-page">Rows per page</span>
          <Select value={String(pageSize)} onValueChange={(value) => onPageSizeChange(Number(value))}>
            <SelectTrigger className="h-8 w-[4.5rem] text-app-caption" aria-labelledby="rows-per-page">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {sizes.map((size) => (
                <SelectItem key={size} value={String(size)}>
                  {size}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="flex items-center gap-1">
          <Button variant="outline" size="icon" className="h-8 w-8" aria-label="First page" disabled={page <= 1} onClick={() => onPageChange(1)}>
            <ChevronsLeft size={15} />
          </Button>
          <Button variant="outline" size="icon" className="h-8 w-8" aria-label="Previous page" disabled={page <= 1} onClick={() => onPageChange(page - 1)}>
            <ChevronLeft size={15} />
          </Button>
          <span className="flex items-center gap-1.5 px-1">
            Page
            <Input
              inputMode="numeric"
              aria-label="Page number"
              value={draft}
              onChange={(event) => setDraft(event.target.value.replace(/\D/g, ''))}
              onBlur={commit}
              onKeyDown={(event) => event.key === 'Enter' && event.currentTarget.blur()}
              className="h-8 w-12 px-1 text-center text-app-caption"
            />
            of {pageCount}
            {lastKnown}
          </span>
          <Button
            variant="outline"
            size="icon"
            className="h-8 w-8"
            aria-label="Next page"
            disabled={atEnd && !hasMore}
            onClick={() => onPageChange(page + 1)}
          >
            <ChevronRight size={15} />
          </Button>
          <Button
            variant="outline"
            size="icon"
            className="h-8 w-8"
            aria-label="Last page"
            disabled={atEnd || hasMore}
            onClick={() => onPageChange(pageCount)}
          >
            <ChevronsRight size={15} />
          </Button>
        </div>
      </div>
    </div>
  )
}
