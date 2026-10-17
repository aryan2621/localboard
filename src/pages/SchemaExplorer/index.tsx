import { useState, type ReactNode } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { AlertTriangle, ArrowLeft, ArrowRight, MessageSquarePlus, SquareTerminal, Table2 } from 'lucide-react'
import { toast } from 'sonner'
import { SampleTable } from '../../components/data/SampleTable'
import { DetailHeader, EmptyState, ListItemCard, PageContainer, PageHeader, PageLoader, SearchInput, SettingsSection, ViewToggle } from '../../components/layout/ui'
import { Alert, AlertDescription } from '../../components/ui/alert'
import { Badge } from '../../components/ui/badge'
import { Button } from '../../components/ui/button'
import { Input } from '../../components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../../components/ui/select'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '../../components/ui/table'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../../components/ui/tabs'
import { PaginationBar } from '../../components/ui/pagination'
import { Textarea } from '../../components/ui/textarea'
import { Hint } from '../../components/ui/tooltip'
import { api, errorMessage, type ColumnRecord, type TableDetail, type TableRecord, type TableUsage } from '../../lib/api'
import { usePagination } from '../../lib/usePagination'
import { useViewMode } from '../../lib/useViewMode'
import { iconStroke } from '../../lib/icon'
import { invalidateWorkspaceData, queryClient, queryKeys } from '../../lib/queryClient'
import { useNavigation } from '../../stores/navigation'
import { Meter, StatRow } from '../../components/layout/patterns'

const DESCRIPTION_LIMIT = 2000

function rows(count: number | null) {
  return count === null ? 'not profiled' : `${count.toLocaleString()} ${count === 1 ? 'row' : 'rows'}`
}

/** A text field that saves on blur or Enter and reverts if the save fails. */
function InlineEdit({
  value,
  onSave,
  multiline,
  placeholder,
  className,
  label,
}: {
  value: string
  onSave: (next: string) => Promise<unknown>
  multiline?: boolean
  placeholder?: string
  className?: string
  label: string
}) {
  const [draft, setDraft] = useState(value)
  const [lastValue, setLastValue] = useState(value)
  if (value !== lastValue) {
    setLastValue(value)
    setDraft(value)
  }
  const commit = () => {
    if (draft.trim() === value.trim()) return
    onSave(draft.trim()).catch((failure) => {
      toast.error(errorMessage(failure))
      setDraft(value)
    })
  }
  const shared = {
    'aria-label': label,
    value: draft,
    placeholder,
    maxLength: DESCRIPTION_LIMIT,
    onChange: (event: { target: { value: string } }) => setDraft(event.target.value),
    onBlur: commit,
  }
  if (multiline) {
    return (
      <Textarea
        {...shared}
        rows={2}
        className={`resize-y border-border bg-surface hover:border-foreground/30 ${className ?? ''}`}
      />
    )
  }
  return (
    <Input
      {...shared}
      onKeyDown={(event) => event.key === 'Enter' && event.currentTarget.blur()}
      className={`border-border bg-surface shadow-none hover:border-foreground/30 ${className ?? ''}`}
    />
  )
}

function ColumnRow({ tableId, column, rowCount, relation }: { tableId: string; column: ColumnRecord; rowCount: number | null; relation: string }) {
  const profile = column.profile
  const unique = profile && rowCount !== null && rowCount > 0 && profile.distinct_count === rowCount && profile.null_pct === 0
  return (
    <TableRow className="align-top">
      <TableCell>
        <div className="flex items-center gap-1.5 font-mono text-app-caption text-foreground">
          {column.name}
          {unique && <Badge variant="secondary">unique</Badge>}
        </div>
        <div className="mt-0.5 font-mono text-app-caption text-muted-foreground">{column.data_type}</div>
      </TableCell>
      <TableCell className="text-app-caption tabular-nums text-muted-foreground">
        {profile ? (
          <div className="w-36 space-y-1">
            <Meter
              value={100 - profile.null_pct}
              label={`${(100 - profile.null_pct).toFixed(0)}%`}
              tone={profile.null_pct > 20 ? 'warning' : 'success'}
            />
            <div>{profile.distinct_count.toLocaleString()} distinct</div>
          </div>
        ) : (
          '—'
        )}
      </TableCell>
      <TableCell className="max-w-[16rem] text-app-caption text-muted-foreground">
        {profile?.min_value !== null && profile?.min_value !== undefined && (
          <Hint text={`${profile.min_value} … ${profile.max_value}`}>
            <div className="truncate font-mono">
              {profile.min_value} … {profile.max_value}
            </div>
          </Hint>
        )}
        <div className="mt-1 flex flex-wrap gap-1">
          {profile?.top_values.map((top) => (
            <Hint key={top.value} text={top.value}>
              <Badge variant="secondary" className="max-w-[10rem] truncate font-mono">
                {top.value} <span className="ml-1 text-muted-foreground">×{top.count}</span>
              </Badge>
            </Hint>
          ))}
        </div>
      </TableCell>
      <TableCell className="w-[34%]">
        <InlineEdit
          label={`Description of ${column.name}`}
          value={column.description}
          placeholder="What this column means…"
          className="h-9 text-app-caption"
          onSave={async (next) => {
            await api.describeColumn(tableId, column.name, next)
            await queryClient.invalidateQueries({ queryKey: queryKeys.table(tableId) })
          }}
        />
      </TableCell>
      <TableCell>
        <ColumnMaskSelect relation={relation} column={column.name} />
      </TableCell>
    </TableRow>
  )
}

function ColumnMaskSelect({ relation, column }: { relation: string; column: string }) {
  const { data: masks = [] } = useQuery({ queryKey: queryKeys.masks, queryFn: api.masks })
  const current = masks.find((mask) => mask.relation === relation && mask.columnName === column)
  const save = useMutation({
    mutationFn: async (strategy: string) => {
      if (strategy === 'none') {
        if (current) await api.maskDelete(current.id)
        return
      }
      await api.maskSave({ relation, columnName: column, strategy })
    },
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.masks }),
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  return (
    <Select value={current?.strategy ?? 'none'} onValueChange={(value) => save.mutate(value)}>
      <SelectTrigger aria-label={`Mask ${column}`} className="w-28">
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value="none">None</SelectItem>
        <SelectItem value="redact">Redact</SelectItem>
        <SelectItem value="hash">Hash</SelectItem>
        <SelectItem value="email">Email</SelectItem>
      </SelectContent>
    </Select>
  )
}

function Relationships({ detail }: { detail: TableDetail }) {
  const openTable = useNavigation((state) => state.openTable)
  const pg = usePagination(detail.relationships, { initialPageSize: 10 })
  if (detail.relationships.length === 0) return null
  return (
    <SettingsSection title="Relationships" description="Detected from column names and matching values. Used to join tables in answers.">
      <div className="space-y-2">
        {pg.pageItems.map((relation) => (
          <ListItemCard
            key={`${relation.direction}-${relation.column}-${relation.otherTableId}-${relation.otherColumn}`}
            onClick={() => openTable(relation.otherTableId)}
            title={
              <span className="flex items-center gap-2 font-mono text-app-caption">
                {relation.direction === 'outgoing' ? (
                  <ArrowRight size={14} strokeWidth={iconStroke} className="text-muted-foreground" />
                ) : (
                  <ArrowLeft size={14} strokeWidth={iconStroke} className="text-muted-foreground" />
                )}
                <span className="text-foreground">{relation.column}</span>
                <span className="font-sans font-normal text-muted-foreground">{relation.direction === 'outgoing' ? 'references' : 'referenced by'}</span>
                <span className="truncate text-foreground">
                  {relation.otherRelation}.{relation.otherColumn}
                </span>
              </span>
            }
            badge={<Badge variant="secondary">{Math.round(relation.confidence * 100)}% match</Badge>}
          />
        ))}
      </div>
      <PaginationBar page={pg.page} pageSize={pg.pageSize} total={pg.total} onPageChange={pg.setPage} onPageSizeChange={pg.setPageSize} className="pt-1" />
    </SettingsSection>
  )
}

function UsedBy({ usage }: { usage: TableUsage }) {
  const openConversation = useNavigation((state) => state.openConversation)
  const openDashboard = useNavigation((state) => state.openDashboard)
  const openDatasite = useNavigation((state) => state.openDatasite)
  const openReport = useNavigation((state) => state.openReport)
  const openDeck = useNavigation((state) => state.openDeck)
  const entries: { key: string; node: ReactNode }[] = [
    ...usage.chats.map((chat) => ({ key: `chat-${chat.id}`, node: <ListItemCard key={`chat-${chat.id}`} title={chat.name} meta="Chat" onClick={() => openConversation(chat.id)} /> })),
    ...usage.savedQueries.map((query) => ({ key: `saved-${query.id}`, node: <ListItemCard key={`saved-${query.id}`} title={query.name} meta="Saved query" /> })),
    ...usage.tiles.map((tile) => ({ key: `tile-${tile.id}`, node: <ListItemCard key={`tile-${tile.id}`} title={tile.name} meta={`Tile · ${tile.dashboardName}`} onClick={() => openDashboard(tile.dashboardId)} /> })),
    ...(usage.datasites ?? []).map((item) => ({ key: `site-${item.id}`, node: <ListItemCard key={`site-${item.id}`} title={item.name} meta={`Datasite · ${item.parentName}`} onClick={() => openDatasite(item.parentId)} /> })),
    ...(usage.reports ?? []).map((item) => ({ key: `report-${item.id}`, node: <ListItemCard key={`report-${item.id}`} title={item.name} meta={`Report · ${item.parentName}`} onClick={() => openReport(item.parentId)} /> })),
    ...(usage.decks ?? []).map((item) => ({ key: `deck-${item.id}`, node: <ListItemCard key={`deck-${item.id}`} title={item.name} meta={`Deck · ${item.parentName}`} onClick={() => openDeck(item.parentId)} /> })),
    ...usage.automations.map((job) => ({ key: `auto-${job.id}`, node: <ListItemCard key={`auto-${job.id}`} title={job.name} meta="Automation" /> })),
  ]
  const pg = usePagination(entries, { initialPageSize: 10 })
  const empty =
    usage.chats.length === 0 &&
    usage.savedQueries.length === 0 &&
    usage.tiles.length === 0 &&
    usage.automations.length === 0 &&
    (usage.datasites?.length ?? 0) === 0 &&
    (usage.reports?.length ?? 0) === 0 &&
    (usage.decks?.length ?? 0) === 0
  return (
    <SettingsSection title="Used by" description="Chats, saved queries, tiles, datasites, reports, decks and automations that read this table after lineage shipped.">
      {usage.lastAuditAt && (
        <p className="mb-2 text-app-caption text-muted-foreground">Last audit {new Date(usage.lastAuditAt).toLocaleString()}</p>
      )}
      {empty ? (
        <EmptyState title="Nothing recorded" description="No chats, tiles or other items have read this table since lineage shipped." />
      ) : (
        <div className="space-y-2">
          {pg.pageItems.map((entry) => entry.node)}
        </div>
      )}
      <PaginationBar page={pg.page} pageSize={pg.pageSize} total={pg.total} onPageChange={pg.setPage} onPageSizeChange={pg.setPageSize} className="pt-1" />
    </SettingsSection>
  )
}

function ColumnsSection({ tableId, columns, rowCount, relation }: { tableId: string; columns: ColumnRecord[]; rowCount: number | null; relation: string }) {
  const [filter, setFilter] = useState('')
  const term = filter.trim().toLowerCase()
  const shown = term ? columns.filter((column) => `${column.name} ${column.data_type} ${column.description}`.toLowerCase().includes(term)) : columns
  const pg = usePagination(shown, { initialPageSize: 25, resetKey: term })
  return (
    <div className="space-y-3">
      {columns.length > 8 && <SearchInput value={filter} onChange={setFilter} placeholder="Filter columns" className="max-w-sm" />}
      {shown.length === 0 && <p className="text-app-label text-muted-foreground">No columns match “{filter}”.</p>}
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Column</TableHead>
            <TableHead>Completeness</TableHead>
            <TableHead>Range and common values</TableHead>
            <TableHead>Description</TableHead>
            <TableHead>Mask</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {pg.pageItems.map((column) => (
            <ColumnRow key={column.name} tableId={tableId} column={column} rowCount={rowCount} relation={relation} />
          ))}
        </TableBody>
      </Table>
      <PaginationBar page={pg.page} pageSize={pg.pageSize} total={pg.total} onPageChange={pg.setPage} onPageSizeChange={pg.setPageSize} className="pt-1" />
    </div>
  )
}

function TableView({ id }: { id: string }) {
  const navigate = useNavigation((state) => state.navigate)
  const openBench = useNavigation((state) => state.openBench)
  const { data: detail, error } = useQuery({ queryKey: queryKeys.table(id), queryFn: () => api.table(id) })
  const rename = useMutation({
    mutationFn: (name: string) => api.renameTable(id, name),
    onSuccess: () => invalidateWorkspaceData(),
  })

  if (error) return <p className="text-app-label text-destructive">{errorMessage(error)}</p>
  if (!detail) {
    return <PageLoader label="Loading table" />
  }

  const { table, columns } = detail
  return (
    <div className="space-y-8">
      <header className="space-y-2">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <InlineEdit
            label="SQL name"
            value={table.relation}
            className="h-11 min-w-0 flex-1 font-mono text-app-heading"
            onSave={(next) => rename.mutateAsync(next)}
          />
          <div className="flex shrink-0 items-center gap-2 pt-1">
            <Button variant="outline" size="sm" onClick={() => openBench(`SELECT *\nFROM ${table.relation}\nLIMIT 100`)}>
              <SquareTerminal strokeWidth={iconStroke} /> Open in Query Bench
            </Button>
            <Button variant="outline" size="sm" onClick={() => navigate('home')}>
              <MessageSquarePlus strokeWidth={iconStroke} /> Ask about this table
            </Button>
          </div>
        </div>
        <Hint text={table.source_ref}>
          <p className="truncate font-mono text-app-caption text-muted-foreground">{table.source_ref}</p>
        </Hint>
        <p className="text-app-caption text-muted-foreground">
          {rows(table.row_count)} · {columns.length} columns
          {table.profiled_at ? ` · profiled ${new Date(/(Z|[+-]\d\d:?\d\d)$/.test(table.profiled_at) ? table.profiled_at : `${table.profiled_at}Z`).toLocaleString()}` : ''}
        </p>
        {table.profile_error && (
          <Alert variant="warning">
            <AlertTriangle size={16} strokeWidth={iconStroke} />
            <AlertDescription>{table.profile_error}</AlertDescription>
          </Alert>
        )}
        <InlineEdit
          multiline
          label="Table description"
          value={table.description}
          placeholder="Describe this table so answers use it correctly…"
          onSave={async (next) => {
            await api.describeTable(id, next)
            await queryClient.invalidateQueries({ queryKey: queryKeys.table(id) })
          }}
        />
      </header>

      <StatRow
        stats={[
          { label: 'Rows', value: table.row_count === null ? '—' : table.row_count.toLocaleString() },
          { label: 'Columns', value: columns.length },
          {
            label: 'Complete',
            value: columns.length
              ? `${Math.round(columns.reduce((sum, column) => sum + (100 - (column.profile?.null_pct ?? 0)), 0) / columns.length)}%`
              : '—',
            hint: 'Average share of non-empty values',
          },
          { label: 'Documented', value: `${columns.filter((column) => column.description).length}/${columns.length}`, hint: 'Columns with a description' },
        ]}
      />
      <Tabs defaultValue="overview">
        <TabsList variant="underline">
          <TabsTrigger value="overview" variant="underline">
            Overview
          </TabsTrigger>
          <TabsTrigger value="columns" variant="underline">
            Columns ({columns.length})
          </TabsTrigger>
          <TabsTrigger value="relationships" variant="underline">
            Relationships
          </TabsTrigger>
          <TabsTrigger value="usage" variant="underline">
            Used by
          </TabsTrigger>
        </TabsList>
        <TabsContent value="overview">
          <SettingsSection title="Sample rows">
            <SampleTable columns={columns.map((column) => column.name)} rows={detail.samples} />
          </SettingsSection>
        </TabsContent>
        <TabsContent value="columns">
          <ColumnsSection tableId={id} columns={columns} rowCount={table.row_count} relation={table.relation} />
        </TabsContent>
        <TabsContent value="relationships">
          <Relationships detail={detail} />
        </TabsContent>
        <TabsContent value="usage">
          <UsedBy usage={detail.usedBy} />
        </TabsContent>
      </Tabs>
    </div>
  )
}

function TableOverview({ tables }: { tables: TableRecord[] }) {
  const openTable = useNavigation((state) => state.openTable)
  const [search, setSearch] = useState('')
  const [view, setView] = useViewMode('schema')
  const { data: connections = [] } = useQuery({ queryKey: queryKeys.connections, queryFn: api.connections })
  const sourceName = (id: string) => connections.find((connection) => connection.id === id)?.name ?? 'Other'
  const term = search.trim().toLowerCase()
  const shown = (term ? tables.filter((table) => `${table.relation} ${table.description}`.toLowerCase().includes(term)) : tables)
    .slice()
    .sort((a, b) => a.relation.localeCompare(b.relation))
  const pg = usePagination(shown, { initialPageSize: 12, resetKey: term })
  const undocumented = tables.filter((table) => !table.description).length
  const status = (table: TableRecord) =>
    table.profile_error ? <Badge variant="warning">Could not profile</Badge> : !table.description ? <Badge variant="outline">No description</Badge> : undefined

  return (
    <div className="space-y-5">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <SearchInput value={search} onChange={setSearch} placeholder="Search tables" className="w-full max-w-sm" />
        <ViewToggle value={view} onChange={setView} />
      </div>
      {undocumented > 0 && (
        <p className="text-app-caption text-muted-foreground">
          {undocumented} {undocumented === 1 ? 'table has' : 'tables have'} no description. Descriptions help the AI pick the right table.
        </p>
      )}
      {shown.length === 0 ? (
        <p className="text-app-label text-muted-foreground">No tables match “{search}”.</p>
      ) : view === 'cards' ? (
        <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
          {pg.pageItems.map((table) => (
            <ListItemCard
              key={table.id}
              title={<span className="font-mono">{table.relation}</span>}
              meta={`${sourceName(table.connection_id)} · ${rows(table.row_count)}${table.description ? ` · ${table.description}` : ''}`}
              badge={status(table)}
              onClick={() => openTable(table.id)}
            />
          ))}
        </div>
      ) : (
        <div className="overflow-hidden rounded-xl border border-border bg-surface">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Table</TableHead>
                <TableHead>Source</TableHead>
                <TableHead className="text-right">Rows</TableHead>
                <TableHead>Description</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {pg.pageItems.map((table) => (
                <TableRow key={table.id} className="cursor-pointer" onClick={() => openTable(table.id)}>
                  <TableCell className="font-mono text-foreground">
                    <button type="button" className="rounded text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40" onClick={() => openTable(table.id)}>
                      {table.relation}
                    </button>
                  </TableCell>
                  <TableCell className="text-muted-foreground">{sourceName(table.connection_id)}</TableCell>
                  <TableCell className="text-right tabular-nums">{table.row_count?.toLocaleString() ?? '–'}</TableCell>
                  <TableCell className="max-w-0 w-[40%]">
                    {table.description ? <span className="block truncate text-muted-foreground">{table.description}</span> : status(table)}
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      )}
      <PaginationBar alwaysShow page={pg.page} pageSize={pg.pageSize} total={pg.total} onPageChange={pg.setPage} onPageSizeChange={pg.setPageSize} />
    </div>
  )
}

export function SchemaExplorerPage() {
  const { data: tables, error } = useQuery({ queryKey: queryKeys.tables, queryFn: api.tables })
  const selectedTableId = useNavigation((state) => state.selectedTableId)
  const schemaSource = useNavigation((state) => state.schemaSource)
  const navigate = useNavigation((state) => state.navigate)
  const selected = tables?.find((table) => table.id === selectedTableId)
  const { data: connections = [] } = useQuery({ queryKey: queryKeys.connections, queryFn: api.connections, enabled: Boolean(schemaSource) })
  const sourceName = connections.find((connection) => connection.id === schemaSource)?.name
  const listed = schemaSource ? (tables ?? []).filter((table) => table.connection_id === schemaSource) : (tables ?? [])

  if (error) return <p className="p-8 text-app-label text-destructive">{errorMessage(error)}</p>
  if (!tables) return <PageLoader label="Loading tables" />
  if (tables.length === 0)
    return (
      <PageContainer>
        <PageHeader title="Schema" description="Tables from your connected data." />
        <EmptyState
          icon={Table2}
          title="No tables yet"
          description="Add a file, folder or database and its tables appear here."
          action={<Button onClick={() => navigate('connections')}>Add data</Button>}
        />
      </PageContainer>
    )

  if (selected) {
    return (
      <>
        <DetailHeader
          title={<span className="text-app-label text-muted-foreground">{schemaSource ? (sourceName ?? 'Source') : 'Schema'}</span>}
          onBack={() => useNavigation.setState({ selectedTableId: null })}
        />
        <div className="mx-auto max-w-6xl space-y-6 px-8 py-8 md:px-12">
          <TableView key={selected.id} id={selected.id} />
        </div>
      </>
    )
  }

  return (
    <PageContainer width="full">
      {schemaSource && (
        <Button variant="ghost" size="sm" className="-ml-2 w-fit" onClick={() => navigate('schema')}>
          <ArrowLeft strokeWidth={iconStroke} /> All tables
        </Button>
      )}
      <PageHeader
        title={schemaSource ? (sourceName ?? 'Source') : 'Schema'}
        description={`${listed.length} ${listed.length === 1 ? 'table' : 'tables'}. Names and descriptions you add here help the AI answer correctly.`}
      />
      {listed.length === 0 ? (
        <EmptyState
          icon={Table2}
          title="No tables from this source"
          description="Open the source in Connections and add a table."
          action={<Button onClick={() => navigate('connections')}>Go to Connections</Button>}
        />
      ) : (
        <TableOverview tables={listed} />
      )}
    </PageContainer>
  )
}
