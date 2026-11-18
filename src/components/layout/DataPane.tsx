import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { ChevronRight, Plus } from 'lucide-react'
import { api } from '../../lib/api'
import { iconStroke } from '../../lib/icon'
import { queryKeys } from '../../lib/queryClient'
import { cn } from '../../lib/utils'
import { useNavigation } from '../../stores/navigation'

function TableNode({ relation, columns }: { relation: string; columns: string[] }) {
  const [open, setOpen] = useState(false)
  const openBench = useNavigation((state) => state.openBench)
  return (
    <div>
      <div className="group flex h-7 items-center rounded-md hover:bg-accent/60">
        <button
          type="button"
          onClick={() => setOpen((current) => !current)}
          aria-expanded={open}
          className="flex min-w-0 flex-1 items-center gap-1 rounded-md px-1.5 text-left text-app-label text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          <ChevronRight size={12} strokeWidth={iconStroke} className={cn('shrink-0 text-muted-foreground transition-transform duration-fast', open && 'rotate-90')} />
          <span className="truncate text-app-body" title={relation}>{relation}</span>
        </button>
        <button
          type="button"
          aria-label={`Query ${relation}`}
          title="Open in Query Bench"
          onClick={() => openBench(`SELECT * FROM ${relation} LIMIT 100`)}
          className="mr-1 hidden h-5 w-5 items-center justify-center rounded text-muted-foreground hover:text-foreground focus-visible:flex focus-visible:ring-2 focus-visible:ring-ring group-hover:flex"
        >
          <Plus size={12} strokeWidth={iconStroke} />
        </button>
      </div>
      {open && (
        <ul className="ml-4 border-l border-border pl-2">
          {columns.map((column) => (
            <li key={column} className="truncate py-0.5 text-app-body text-muted-foreground" title={column}>{column}</li>
          ))}
        </ul>
      )}
    </div>
  )
}

/** Left pane of the workbench: the tables you can ask about. */
export function DataPane() {
  const { data: tables = [] } = useQuery({ queryKey: queryKeys.benchSchema, queryFn: api.benchSchema })
  return (
    <aside className="flex w-60 shrink-0 flex-col border-r border-border bg-sidebar">
      <div className="mac-window-space mac-drag-region shrink-0" />
      <div className="min-h-0 flex-1 overflow-y-auto px-2 py-3">
        <p className="px-1.5 pb-1 text-app-caption font-medium text-muted-foreground">Tables</p>
        {tables.length === 0 && <p className="px-1.5 text-app-caption text-muted-foreground">No data yet.</p>}
        {tables.map((table) => (
          <TableNode key={table.relation} {...table} />
        ))}
      </div>
    </aside>
  )
}
