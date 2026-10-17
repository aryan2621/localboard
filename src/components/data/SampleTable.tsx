import type { SampleRow } from '../../lib/api'
import { PaginationBar } from '../ui/pagination'
import { usePagination } from '../../lib/usePagination'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '../ui/table'

export function formatCell(value: unknown) {
  if (value === null || value === undefined) return '∅'
  if (typeof value === 'object') return JSON.stringify(value)
  return String(value)
}

export function SampleTable({ columns, rows }: { columns: string[]; rows: SampleRow[] }) {
  const pg = usePagination(rows, { initialPageSize: 10, resetKey: rows })
  if (rows.length === 0) return <p className="text-app-label text-muted-foreground">No rows to preview.</p>
  return (
    <div className="space-y-1">
    <div className="max-h-80 overflow-auto rounded-xl">
      <Table className="font-mono text-app-caption">
        <TableHeader>
          <TableRow>
            {columns.map((column) => (
              <TableHead key={column} className="whitespace-nowrap">
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
                    className={value === null || value === undefined ? 'text-muted-foreground/60' : 'max-w-[18rem] truncate text-foreground'}
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
