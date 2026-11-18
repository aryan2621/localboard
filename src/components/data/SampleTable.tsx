import { cn } from '../../lib/utils'
import type { SampleRow } from '../../lib/api'
import { PaginationBar } from '../ui/pagination'
import { usePagination } from '../../lib/usePagination'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '../ui/table'

export function formatCell(value: unknown) {
  if (value === null || value === undefined) return '∅'
  if (typeof value === 'object') return JSON.stringify(value)
  return String(value)
}

/** Columns whose non-empty values are all numbers; shown right-aligned. */
export function numericColumns(columns: string[], rows: SampleRow[]) {
  const isNum = (v: unknown) => typeof v === 'number' || (typeof v === 'string' && /^-?\d+(\.\d+)?$/.test(v.trim()))
  return new Set(columns.filter((c) => { const vals = rows.map((r) => r[c]).filter((v) => v !== null && v !== undefined); return vals.length > 0 && vals.every(isNum) }))
}

export function SampleTable({ columns, rows }: { columns: string[]; rows: SampleRow[] }) {
  const pg = usePagination(rows, { initialPageSize: 10, resetKey: rows })
  const numeric = numericColumns(columns, rows)
  if (rows.length === 0) return <p className="text-app-label text-muted-foreground">No rows to preview.</p>
  return (
    <div className="space-y-1">
    <div className="max-h-80 overflow-auto rounded-lg">
      <Table>
        <TableHeader>
          <TableRow>
            {columns.map((column) => (
              <TableHead key={column} className={numeric.has(column) ? 'whitespace-nowrap text-right' : 'whitespace-nowrap'}>
                {column}
              </TableHead>
            ))}
          </TableRow>
        </TableHeader>
        <TableBody>
          {pg.pageItems.map((row, index) => (
            <TableRow key={(pg.page - 1) * pg.pageSize + index}>
              {columns.map((column) => {
                const value = row[column]
                return (
                  <TableCell
                    key={column}
                    className={cn(numeric.has(column) && 'text-right font-mono text-app-label', value === null || value === undefined ? 'text-muted-foreground/60' : 'max-w-[18rem] truncate text-foreground')}
                  >
                    {formatCell(value)}
                  </TableCell>
                )
              })}
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
    <PaginationBar page={pg.page} pageSize={pg.pageSize} total={pg.total} onPageChange={pg.setPage} onPageSizeChange={pg.setPageSize} className="pt-1" />
    </div>
  )
}
