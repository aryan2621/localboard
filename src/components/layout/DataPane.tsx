import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { ChevronRight, MessageSquarePlus, Plus } from 'lucide-react'
import { api } from '../../lib/api'
import { iconStroke } from '../../lib/icon'
import { queryKeys } from '../../lib/queryClient'
import { cn } from '../../lib/utils'
import { useNavigation } from '../../stores/navigation'
import { AiStatus, ChatHistory } from './Sidebar'

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
          <span className="truncate font-mono text-app-caption" title={relation}>{relation}</span>
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
            <li key={column} className="truncate py-0.5 font-mono text-app-caption text-muted-foreground" title={column}>{column}</li>
          ))}
        </ul>
      )}
    </div>
  )
}

/** Left pane of the workbench: what you can ask about, and what you already asked. */
export function DataPane() {
  const navigate = useNavigation((state) => state.navigate)
  const { data: tables = [] } = useQuery({ queryKey: queryKeys.benchSchema, queryFn: api.benchSchema })
  return (
    <aside className="flex w-60 shrink-0 flex-col border-r border-border bg-sidebar">
      <div className="mac-window-space mac-drag-region shrink-0" />
      <div className="flex items-center justify-between px-3 pb-2 pt-3">
        <h2 className="text-app-label font-medium text-foreground">Workbench</h2>
        <button
          type="button"
          onClick={() => navigate('home')}
          className="flex h-6 items-center gap-1 rounded-md px-1.5 text-app-caption text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          <MessageSquarePlus size={13} strokeWidth={iconStroke} /> New chat
        </button>
      </div>
      <div className="max-h-[45%] min-h-0 shrink-0 overflow-y-auto px-2 pb-2">
        <p className="px-1.5 pb-1 text-app-caption font-medium text-muted-foreground">Tables</p>
        {tables.length === 0 && <p className="px-1.5 text-app-caption text-muted-foreground">No data yet.</p>}
        {tables.map((table) => (
          <TableNode key={table.relation} {...table} />
        ))}
      </div>
      <div className="flex min-h-0 flex-1 flex-col px-2">
        <ChatHistory />
      </div>
      <div className="border-t border-border px-2 py-2">
        <AiStatus />
      </div>
    </aside>
  )
}
