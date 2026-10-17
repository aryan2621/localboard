import { useEffect, useMemo, useRef, useState } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { save } from '@tauri-apps/plugin-dialog'
import { AlertTriangle, ChevronDown, History, Play, Sparkles, X, Table2, Bookmark } from 'lucide-react'
import { toast } from 'sonner'
import { SqlEditor } from '../../components/bench/SqlEditor'
import { Button } from '../../components/ui/button'
import { Sheet, SheetBody, SheetContent, SheetHeader } from '../../components/ui/sheet'
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger } from '../../components/ui/dropdown-menu'
import { Badge } from '../../components/ui/badge'
import { PaginationBar } from '../../components/ui/pagination'
import { usePagination } from '../../lib/usePagination'
import { Input } from '../../components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../../components/ui/select'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '../../components/ui/table'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../../components/ui/tabs'
import { Alert, AlertDescription } from '../../components/ui/alert'
import { ConfirmDialog, EmptyState, FormField, ListItemCard, RowMenu, SearchInput, StatusBadge } from '../../components/layout/ui'
import { Kbd } from '../../components/layout/patterns'
import { api, errorMessage, type QueryResult } from '../../lib/api'
import { iconStroke } from '../../lib/icon'
import { queryClient, queryKeys } from '../../lib/queryClient'
import { relativeTime } from '../../lib/time'
import { useNavigation } from '../../stores/navigation'

function namesIn(sql: string) {
  const found = new Set<string>()
  const pattern = /:(?!:)([A-Za-z_][A-Za-z0-9_]*)/g
  for (const match of sql.matchAll(pattern)) found.add(match[1])
  return [...found]
}

export function QueryBenchPage() {
  const schemaQuery = useQuery({ queryKey: queryKeys.benchSchema, queryFn: api.benchSchema })
  const history = useQuery({ queryKey: queryKeys.benchHistory, queryFn: api.benchHistory })
  const saved = useQuery({ queryKey: queryKeys.benchSaved, queryFn: api.benchSaved })
  const agents = useQuery({ queryKey: queryKeys.agents, queryFn: api.agents })
  const dashboards = useQuery({ queryKey: queryKeys.dashboards, queryFn: api.dashboards })
  const benchSql = useNavigation((state) => state.benchSql)
  const navigate = useNavigation((state) => state.navigate)
  const schema = useMemo(() => {
    const tables: Record<string, string[]> = {}
    for (const table of schemaQuery.data ?? []) tables[table.relation] = table.columns
    return tables
  }, [schemaQuery.data])

  const [sql, setSql] = useState(benchSql || 'SELECT 1')

  useEffect(() => {
    if (benchSql) setSql(benchSql)
  }, [benchSql])
  const [rangeName, setRangeName] = useState('')
  const [values, setValues] = useState<Record<string, string>>({})
  const [result, setResult] = useState<QueryResult | null>(null)
  const [rendered, setRendered] = useState('')
  const [note, setNote] = useState('')
  const [filter, setFilter] = useState('')
  const [sort, setSort] = useState<{ column: number; desc: boolean } | null>(null)
  const [name, setName] = useState('')
  const [question, setQuestion] = useState('')
  const [agentId, setAgentId] = useState('')
  const [dashboardId, setDashboardId] = useState('')
  const [tileTitle, setTileTitle] = useState('')
  const [lastError, setLastError] = useState('')
  const [explanation, setExplanation] = useState('')
  const [editingSaved, setEditingSaved] = useState<{ id: string; name: string } | null>(null)
  const [historySearch, setHistorySearch] = useState('')
  const [removing, setRemoving] = useState<{ id: string; name: string } | null>(null)
  const [drawer, setDrawer] = useState<'tables' | 'history' | 'saved' | null>(null)
  const [tableSearch, setTableSearch] = useState('')
  const insertRef = useRef<((text: string) => void) | null>(null)
  const [dialog, setDialog] = useState<'save' | 'example' | 'tile' | null>(null)

  const params = namesIn(sql)
  const bound = Object.fromEntries(params.map((name) => [name, values[name] ?? '']))

  const refreshLists = () => {
    void queryClient.invalidateQueries({ queryKey: queryKeys.benchHistory })
    void queryClient.invalidateQueries({ queryKey: queryKeys.benchSaved })
  }

  const removeSaved = useMutation({
    mutationFn: (queryId: string) => api.benchDeleteQuery(queryId),
    onSuccess: (_result, queryId) => {
      if (editingSaved?.id === queryId) setEditingSaved(null)
      refreshLists()
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })

  const run = useMutation({
    mutationFn: () => api.benchRun(sql, bound),
    onSuccess: (outcome) => {
      setResult(outcome.result)
      setRendered(outcome.sql)
      setNote(`${outcome.result.rows.length} rows in ${outcome.durationMs} ms`)
      setExplanation('')
      setLastError('')
      refreshLists()
    },
    onError: (failure) => {
      setLastError(errorMessage(failure))
      refreshLists()
    },
  })

  const format = useMutation({
    mutationFn: () => api.benchFormat(sql),
    onSuccess: (text) => setSql(text.sql),
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const explain = useMutation({
    mutationFn: () => api.benchExplain(sql, bound),
    onSuccess: (rows) => {
      setResult(rows)
      setNote('EXPLAIN')
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const fix = useMutation({
    mutationFn: () => api.benchFix(sql, lastError || 'The query failed.'),
    onSuccess: (text) => {
      setSql(text.sql)
      toast.success('Fixed query is in the editor. Run it to check.')
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const optimize = useMutation({
    mutationFn: () => api.benchOptimize(sql),
    onSuccess: (text) => {
      setSql(text.sql)
      toast.success('Optimized query is in the editor.')
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const describe = useMutation({
    mutationFn: () => api.benchDescribe(sql),
    onSuccess: (text) => setExplanation(text.text),
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const saveQuery = useMutation({
    mutationFn: () => api.benchSaveQuery(name, sql, editingSaved?.id),
    onSuccess: (query) => {
      setEditingSaved({ id: query.id, name: query.name })
      setName('')
      toast.success(`Saved “${query.name}”`)
      refreshLists()
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const saveTile = useMutation({
    mutationFn: () => api.dashboardAddTile(dashboardId || dashboards.data?.[0]?.id || '', tileTitle || name || 'Tile', rendered || sql),
    onSuccess: () => {
      toast.success('Saved as a tile')
      void queryClient.invalidateQueries({ queryKey: queryKeys.dashboards })
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const saveExample = useMutation({
    mutationFn: () => api.saveExample(agentId || agents.data?.[0]?.id || '', question, rendered || sql),
    onSuccess: () => {
      setQuestion('')
      toast.success('Saved as an example')
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const exportAs = async (formatName: string) => {
    const path = await save({ defaultPath: `result.${formatName}`, filters: [{ name: formatName, extensions: [formatName] }] })
    if (!path) return
    try {
      await api.benchExport(sql, bound, formatName, path)
      toast.success(`Exported ${formatName}`)
    } catch (failure) {
      toast.error(errorMessage(failure))
    }
  }

  const rows = useMemo(() => {
    if (!result) return []
    const needle = filter.trim().toLowerCase()
    let next = result.rows.map((row, index) => ({ row, index }))
    if (needle) next = next.filter(({ row }) => row.some((cell) => String(cell ?? '').toLowerCase().includes(needle)))
    if (sort) {
      next = [...next].sort((left, right) => {
        const a = String(left.row[sort.column] ?? '')
        const b = String(right.row[sort.column] ?? '')
        const numeric = Number(a) - Number(b)
        const compared = Number.isNaN(numeric) ? a.localeCompare(b) : numeric
        return sort.desc ? -compared : compared
      })
    }
    return next
  }, [result, filter, sort])

  const pg = usePagination(rows, { initialPageSize: 50, resetKey: result ? `${filter}|${sort?.column}|${sort?.desc}|${rows.length}|${result.rows.length}` : filter })
  const historyTerm = historySearch.trim().toLowerCase()
  const historyRows = (history.data ?? []).filter((entry) => !historyTerm || entry.sql.toLowerCase().includes(historyTerm))
  const savedRows = saved.data ?? []
  const historyPg = usePagination(historyRows, { initialPageSize: 10, resetKey: historyTerm })
  const savedPg = usePagination(savedRows, { initialPageSize: 10 })

  const busy = run.isPending || explain.isPending || fix.isPending || optimize.isPending || describe.isPending
  const dirty = Boolean(editingSaved) && sql.trim() !== (savedRows.find((query) => query.id === editingSaved?.id)?.sql ?? '').trim()

  /** Replacing the editor would lose unsaved edits, so ask first. */
  const loadSql = (nextSql: string, saved?: { id: string; name: string }, nextParams?: Record<string, string>) => {
    if (dirty && !window.confirm('Replace the editor contents? Unsaved changes to this query are lost.')) return
    setSql(nextSql)
    setEditingSaved(saved ?? null)
    if (nextParams) setValues((current) => ({ ...current, ...nextParams }))
  }

  const tableTerm = tableSearch.trim().toLowerCase()
  const filteredTables = (schemaQuery.data ?? []).filter((table) => !tableTerm || table.relation.toLowerCase().includes(tableTerm) || table.columns.some((column) => column.toLowerCase().includes(tableTerm)))
  const insertAtCursor = (text: string) => insertRef.current?.(text)

  const restoreHistory = (entry: (typeof historyRows)[number]) => {
    let stored: Record<string, string> | undefined
    try {
      const parsed: unknown = JSON.parse(entry.paramsJson || '{}')
      if (parsed && typeof parsed === 'object') stored = Object.fromEntries(Object.entries(parsed as Record<string, unknown>).map(([key, value]) => [key, String(value ?? '')]))
    } catch {
      stored = undefined
    }
    loadSql(entry.sql, undefined, stored)
    setDrawer(null)
  }

  return (
    <div className="flex flex-col">
      <div className="min-w-0">
            <div className="sticky top-0 z-10 flex items-center justify-between gap-3 border-b border-border bg-background/85 px-8 py-3 backdrop-blur">
              <div className="min-w-0">
                <h1 className="font-serif text-app-title text-foreground">Query Bench</h1>
                <p className="truncate text-app-caption text-muted-foreground">Read-only SELECT. Parameters use :name and are bound, never pasted in.</p>
              </div>
              <span className="hidden shrink-0 items-center gap-1.5 text-app-caption text-muted-foreground md:flex">
                <Kbd>⌘</Kbd>
                <Kbd>↵</Kbd> to run
              </span>
            </div>
        <div className="min-w-0 space-y-4 px-8 py-6">
          {editingSaved && (
            <div className="flex items-center gap-2 text-app-caption text-muted-foreground">
              <span>
                Editing <span className="font-medium text-foreground">{editingSaved.name}</span>
              </span>
              {dirty && <StatusBadge variant="warning">Unsaved</StatusBadge>}
              <Button variant="ghost" size="sm" className="h-6 px-2" onClick={() => setEditingSaved(null)}>
                Stop editing
              </Button>
            </div>
          )}
          <Tabs value={drawer ?? ''} onValueChange={(value) => setDrawer(value as 'tables' | 'history' | 'saved')}>
            <TabsList>
              {[
                { value: 'tables', label: 'Tables', Icon: Table2, count: (schemaQuery.data ?? []).length },
                { value: 'history', label: 'History', Icon: History, count: (history.data ?? []).length },
                { value: 'saved', label: 'Saved', Icon: Bookmark, count: savedRows.length },
              ].map(({ value, label, Icon, count }) => (
                <TabsTrigger key={value} value={value} className="group gap-2">
                  <Icon size={15} strokeWidth={iconStroke} />
                  {label}
                  <span className="rounded-full bg-muted px-1.5 text-app-caption tabular-nums text-muted-foreground group-data-[state=active]:bg-brand/15 group-data-[state=active]:text-brand">
                    {count}
                  </span>
                </TabsTrigger>
              ))}
            </TabsList>
          </Tabs>
          <SqlEditor insertRef={insertRef} value={sql} schema={schema} onChange={setSql} onRun={() => !busy && sql.trim() && run.mutate()} />
          {params.length > 0 && (
            <div className="grid gap-3 sm:grid-cols-2">
              {params.map((name) => (
                <FormField key={name} label={`:${name}`} htmlFor={`qb-param-${name}`}>
                  <Input
                    id={`qb-param-${name}`}
                    value={values[name] ?? ''}
                    placeholder="Value"
                    onChange={(event) => setValues((current) => ({ ...current, [name]: event.target.value }))}
                  />
                </FormField>
              ))}
            </div>
          )}
          <div className="flex flex-wrap items-center gap-2">
            <Button onClick={() => run.mutate()} loading={run.isPending} disabled={busy || !sql.trim()}>
              {!run.isPending && <Play strokeWidth={iconStroke} />} Run
              <kbd className="ml-1 rounded border border-primary-foreground/25 px-1 text-app-caption opacity-70">⌘↵</kbd>
            </Button>
            <Button variant="outline" onClick={() => format.mutate()} disabled={format.isPending}>
              Format
            </Button>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="outline" disabled={busy}>
                  <Sparkles strokeWidth={iconStroke} /> Assist <ChevronDown strokeWidth={iconStroke} />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start">
                <DropdownMenuItem onSelect={() => explain.mutate()}>Show EXPLAIN plan</DropdownMenuItem>
                <DropdownMenuItem onSelect={() => describe.mutate()}>Explain in words</DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem disabled={!lastError} onSelect={() => fix.mutate()}>
                  Fix the last error
                </DropdownMenuItem>
                <DropdownMenuItem onSelect={() => optimize.mutate()}>Optimize query</DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button variant="outline" disabled={!sql.trim()}>
                  Save <ChevronDown strokeWidth={iconStroke} />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start">
                <DropdownMenuItem onSelect={() => setDialog('save')}>{editingSaved ? `Update “${editingSaved.name}”` : 'Save query'}</DropdownMenuItem>
                <DropdownMenuItem onSelect={() => setDialog('example')}>Save as analyst example</DropdownMenuItem>
                <DropdownMenuItem onSelect={() => setDialog('tile')}>Add as dashboard tile</DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
          {lastError && (
            <Alert variant="destructive">
              <AlertTriangle />
              <AlertDescription className="space-y-2">
                <p className="whitespace-pre-wrap">{lastError}</p>
                <Button variant="outline" size="sm" loading={fix.isPending} onClick={() => fix.mutate()}>
                  <Sparkles strokeWidth={iconStroke} /> Fix with AI
                </Button>
              </AlertDescription>
            </Alert>
          )}
          {explanation && (
            <div className="relative rounded-xl border border-border bg-muted/40 px-4 py-3 pr-10 text-app-label text-foreground">
              <p className="mb-1 text-app-caption font-medium text-muted-foreground">What this query does</p>
              {explanation}
              <Button variant="ghost" size="icon" className="absolute right-1.5 top-1.5 h-7 w-7" aria-label="Dismiss explanation" onClick={() => setExplanation('')}>
                <X strokeWidth={iconStroke} />
              </Button>
            </div>
          )}
          {note && <p className="text-app-caption tabular-nums text-muted-foreground">{note}</p>}
          {result ? (
            <div className="space-y-2">
              <div className="flex flex-wrap items-center gap-2">
                <Input value={filter} onChange={(event) => setFilter(event.target.value)} placeholder="Filter rows" aria-label="Filter rows" className="max-w-xs" />
                <div className="ml-auto">
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <Button variant="outline">
                        Export <ChevronDown strokeWidth={iconStroke} />
                      </Button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="end">
                      <DropdownMenuLabel>Export result as</DropdownMenuLabel>
                      {(['csv', 'json', 'xlsx', 'parquet'] as const).map((formatName) => (
                        <DropdownMenuItem key={formatName} onSelect={() => void exportAs(formatName)}>
                          {formatName.toUpperCase()}
                        </DropdownMenuItem>
                      ))}
                    </DropdownMenuContent>
                  </DropdownMenu>
                </div>
              </div>
              <div className="max-h-[32rem] overflow-auto rounded-xl">
                <Table>
                  <TableHeader className="sticky top-0 bg-muted">
                    <TableRow>
                      {result.columns.map((column, index) => (
                        <TableHead key={column.name}>
                          <button
                            type="button"
                            className="flex items-baseline gap-1 font-mono text-app-caption hover:text-foreground"
                            onClick={() => setSort((current) => ({ column: index, desc: current?.column === index ? !current.desc : false }))}
                          >
                            {column.name}
                            <span className="font-sans text-muted-foreground/70">{column.dataType}</span>
                            {sort?.column === index && <span aria-hidden>{sort.desc ? '↓' : '↑'}</span>}
                          </button>
                        </TableHead>
                      ))}
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {pg.pageItems.map(({ row, index }) => (
                      <TableRow key={index}>
                        {row.map((cell, cellIndex) => (
                          <TableCell key={cellIndex} className="px-3 py-1.5 font-mono text-app-caption">
                            {cell == null ? '' : String(cell)}
                          </TableCell>
                        ))}
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </div>
              <PaginationBar page={pg.page} pageSize={pg.pageSize} total={pg.total} onPageChange={pg.setPage} onPageSizeChange={pg.setPageSize} className="pt-1" />
            </div>
          ) : (
            <EmptyState icon={Play} title="No results yet" description="Run a query to see rows here." />
          )}
        </div>
      </div>

      <Sheet open={drawer === 'tables'} onOpenChange={(open) => !open && setDrawer(null)}>
        <SheetContent>
          <SheetHeader title="Tables" description="Click a table or column to insert it at the cursor." />
          <SheetBody>
<SearchInput value={tableSearch} onChange={setTableSearch} placeholder="Search tables and columns" />
              {(schemaQuery.data ?? []).length === 0 && <EmptyState title="No tables" description="Add data in Connections first." />}
              {filteredTables.map((table) => (
                <details key={table.relation} className="group rounded-lg border border-border bg-surface">
                  <summary className="flex cursor-pointer list-none items-center justify-between gap-2 px-3 py-2 [&::-webkit-details-marker]:hidden">
                    <button type="button" onClick={(event) => { event.preventDefault(); insertAtCursor(table.relation) }} className="truncate font-mono text-app-caption text-foreground hover:underline">{table.relation}</button>
                    <ChevronDown size={14} strokeWidth={iconStroke} className="shrink-0 text-muted-foreground transition-transform group-open:rotate-180" />
                  </summary>
                  <div className="flex flex-wrap gap-1 border-t border-border px-3 py-2">
                    {table.columns.map((column) => (
                      <button
                        key={column}
                        type="button"
                        onClick={() => insertAtCursor(column)}
                        className="rounded bg-muted px-1.5 py-0.5 font-mono text-app-caption text-muted-foreground transition-colors duration-fast hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40"
                      >
                        {column}
                      </button>
                    ))}
                  </div>
                </details>
              ))}
          </SheetBody>
        </SheetContent>
      </Sheet>

      <Sheet open={drawer === 'history'} onOpenChange={(open) => !open && setDrawer(null)}>
        <SheetContent>
          <SheetHeader title="History" description="Click a query to load it into the editor." />
          <SheetBody>
              {(history.data ?? []).length === 0 ? (
                <EmptyState icon={History} title="No history yet" description="Queries you run appear here." />
              ) : (
                <SearchInput value={historySearch} onChange={setHistorySearch} placeholder="Search history" />
              )}
              {historyPg.pageItems.map((entry) => (
                <button
                  key={entry.id}
                  type="button"
                  onClick={() => restoreHistory(entry)}
                  className="block w-full rounded-lg border border-border bg-surface px-3 py-2 text-left transition-colors duration-fast hover:border-foreground/25 hover:bg-accent/40 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40"
                >
                  <span className="block truncate font-mono text-app-caption text-foreground">{entry.sql}</span>
                  <span className="mt-0.5 flex flex-wrap items-center gap-x-2 text-app-caption text-muted-foreground">
                    {entry.status !== 'ok' && <Badge variant="destructive">{entry.status}</Badge>}
                    {entry.rowCount !== null && <span className="tabular-nums">{entry.rowCount.toLocaleString()} rows</span>}
                    {entry.durationMs !== null && <span className="tabular-nums">{entry.durationMs} ms</span>}
                    <span>{relativeTime(entry.createdAt)}</span>
                  </span>
                </button>
              ))}
              <PaginationBar page={historyPg.page} pageSize={historyPg.pageSize} total={historyPg.total} onPageChange={historyPg.setPage} onPageSizeChange={historyPg.setPageSize} />
          </SheetBody>
        </SheetContent>
      </Sheet>

      <Sheet open={drawer === 'saved'} onOpenChange={(open) => !open && setDrawer(null)}>
        <SheetContent>
          <SheetHeader title="Saved queries" description="Click a query to load it into the editor." />
          <SheetBody>
              {(saved.data ?? []).length === 0 && <EmptyState title="No saved queries" description="Run a query, then use Save to keep it." />}
              {savedPg.pageItems.map((query) => (
                <div key={query.id} className="group flex items-center gap-1">
                  <ListItemCard
                    className={`min-w-0 flex-1 ${editingSaved?.id === query.id ? 'border-brand/40' : ''}`}
                    title={query.name}
                    meta={`Edited ${relativeTime(query.updatedAt)}`}
                    onClick={() => { loadSql(query.sql, { id: query.id, name: query.name }); setDrawer(null) }}
                  />
                  <RowMenu label={`Actions for ${query.name}`}>
                    <DropdownMenuItem onSelect={() => { loadSql(query.sql, { id: query.id, name: query.name }); setDrawer(null) }}>Open in editor</DropdownMenuItem>
                    <DropdownMenuSeparator />
                    <DropdownMenuItem className="text-destructive focus:text-destructive" onSelect={() => setRemoving({ id: query.id, name: query.name })}>
                      Delete
                    </DropdownMenuItem>
                  </RowMenu>
                </div>
              ))}
              <PaginationBar page={savedPg.page} pageSize={savedPg.pageSize} total={savedPg.total} onPageChange={savedPg.setPage} onPageSizeChange={savedPg.setPageSize} />
          </SheetBody>
        </SheetContent>
      </Sheet>



      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(open) => !open && setRemoving(null)}
        title={`Delete “${removing?.name ?? ''}”?`}
        description="The saved query is removed. Dashboards and automations that already copied its SQL are not affected."
        confirmLabel="Delete query"
        onConfirm={() => removing && removeSaved.mutate(removing.id)}
      />

      <Sheet open={dialog === 'save'} onOpenChange={(open) => !open && setDialog(null)}>
        <SheetContent>
          <SheetHeader title="Save query" description="Keep this SQL under a name so you can reopen it from Saved." />
          <SheetBody>
          <form
            className="space-y-4"
            onSubmit={(event) => {
              event.preventDefault()
              if ((name || editingSaved?.name || '').trim()) {
                if (!name.trim() && editingSaved) setName(editingSaved.name)
                saveQuery.mutate()
                setDialog(null)
              }
            }}
          >
            <FormField label="Name" htmlFor="qb-save-name">
              <Input id="qb-save-name" value={name || editingSaved?.name || ''} onChange={(event) => setName(event.target.value)} placeholder="Save query as" />
            </FormField>
            <Button type="submit" loading={saveQuery.isPending} disabled={!(name || editingSaved?.name || '').trim()}>
              {editingSaved ? 'Update query' : 'Save query'}
            </Button>
          </form>
        
          </SheetBody>
        </SheetContent>
      </Sheet>

      <Sheet open={dialog === 'example'} onOpenChange={(open) => !open && setDialog(null)}>
        <SheetContent>
          <SheetHeader title="Save as analyst example" description="Teach an analyst how to answer a question with this SQL." />
          <SheetBody>
          <form
            className="space-y-4"
            onSubmit={(event) => {
              event.preventDefault()
              if (question.trim()) {
                saveExample.mutate()
                setDialog(null)
              }
            }}
          >
            <FormField label="Analyst">
              <Select value={agentId || undefined} onValueChange={setAgentId}>
                <SelectTrigger aria-label="Analyst">
                  <SelectValue placeholder={agents.data?.[0]?.name ?? 'Analyst'} />
                </SelectTrigger>
                <SelectContent>
                  {(agents.data ?? []).map((agent) => (
                    <SelectItem key={agent.id} value={agent.id}>
                      {agent.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </FormField>
            <FormField label="Question" htmlFor="qb-example-question">
              <Input id="qb-example-question" value={question} onChange={(event) => setQuestion(event.target.value)} placeholder="Question for this SQL" />
            </FormField>
            <Button type="submit" disabled={!question.trim() || saveExample.isPending}>
              Save as example
            </Button>
          </form>
        
          </SheetBody>
        </SheetContent>
      </Sheet>

      <Sheet open={dialog === 'tile'} onOpenChange={(open) => !open && setDialog(null)}>
        <SheetContent>
          <SheetHeader title="Add as dashboard tile" description="Pin this query to a dashboard." />
          <SheetBody>
          <form
            className="space-y-4"
            onSubmit={(event) => {
              event.preventDefault()
              saveTile.mutate()
              setDialog(null)
            }}
          >
            {(dashboards.data?.length ?? 0) === 0 && (
              <EmptyState
                title="No dashboards yet"
                description="Build a dashboard first, then you can pin queries to it."
                action={<Button onClick={() => navigate('dashboards')}>Go to Dashboards</Button>}
              />
            )}
            <FormField label="Dashboard">
              <Select value={dashboardId || undefined} onValueChange={setDashboardId}>
                <SelectTrigger aria-label="Dashboard">
                  <SelectValue placeholder={dashboards.data?.[0]?.name ?? 'Dashboard'} />
                </SelectTrigger>
                <SelectContent>
                  {(dashboards.data ?? []).map((item) => (
                    <SelectItem key={item.id} value={item.id}>
                      {item.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </FormField>
            <FormField label="Tile title" htmlFor="qb-tile-title">
              <Input id="qb-tile-title" value={tileTitle} onChange={(event) => setTileTitle(event.target.value)} placeholder="Tile title" />
            </FormField>
            <Button type="submit" disabled={(dashboards.data?.length ?? 0) === 0 || saveTile.isPending}>
              Save as tile
            </Button>
          </form>
        
          </SheetBody>
        </SheetContent>
      </Sheet>
    </div>
  )
}
