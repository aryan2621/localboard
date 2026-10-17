import { useState, type ReactNode } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { open } from '@tauri-apps/plugin-dialog'
import { BookOpen, Brain, FileText, History, ListChecks, MessageSquarePlus, Plus, ScrollText, Trash2, Wrench } from 'lucide-react'
import { toast } from 'sonner'
import { Button } from '../../components/ui/button'
import { Badge } from '../../components/ui/badge'
import { Checkbox } from '../../components/ui/checkbox'
import { Switch } from '../../components/ui/switch'
import { Input } from '../../components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../../components/ui/select'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../../components/ui/tabs'
import { Textarea } from '../../components/ui/textarea'
import { ConfirmDialog, DetailHeader, EmptyState, FormField, MoreMenu, PageContainer, PageHeader, PageLoader, RowMenu, SearchInput, SectionHeader, SegmentedControl, ViewToggle } from '../../components/layout/ui'
import { Avatar, Banner, IconTile, Panel, SaveBar, StatRow } from '../../components/layout/patterns'
import { FormSheet, Sheet, SheetBody, SheetContent, SheetHeader } from '../../components/ui/sheet'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '../../components/ui/table'
import { useViewMode } from '../../lib/useViewMode'
import { relativeTime } from '../../lib/time'
import { DropdownMenuItem, DropdownMenuSeparator } from '../../components/ui/dropdown-menu'
import { useNavigation } from '../../stores/navigation'
import {
  api,
  errorMessage,
  type Agent,
  type Rule,
  type RuleVersion,
  type TaskRouting,
} from '../../lib/api'
import { PaginationBar } from '../../components/ui/pagination'
import { usePagination } from '../../lib/usePagination'
import { iconStroke } from '../../lib/icon'
import { queryClient, queryKeys } from '../../lib/queryClient'

const DEFAULT = 'default'
const ALL_TABLES = 'all'

function invalidate(agentId?: string) {
  void queryClient.invalidateQueries({ queryKey: queryKeys.agents })
  if (agentId) {
    void queryClient.invalidateQueries({ queryKey: queryKeys.rules(agentId) })
    void queryClient.invalidateQueries({ queryKey: queryKeys.examples(agentId) })
    void queryClient.invalidateQueries({ queryKey: queryKeys.memories(agentId) })
    void queryClient.invalidateQueries({ queryKey: queryKeys.knowledge(agentId) })
  }
}

const TASK_LABEL = { sql: 'Writing SQL', reasoning: 'Thinking it through', narrative: 'Writing summaries' } as const

export function AgentsPage() {
  const { data: agents, error } = useQuery({ queryKey: queryKeys.agents, queryFn: api.agents })
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [search, setSearch] = useState('')
  const [view, setView] = useViewMode('analysts')
  const [newOpen, setNewOpen] = useState(false)
  const [newName, setNewName] = useState('')
  const [newDescription, setNewDescription] = useState('')
  const term = search.trim().toLowerCase()
  const shown = (agents ?? []).filter((agent) => !term || `${agent.name} ${agent.description}`.toLowerCase().includes(term))
  const pg = usePagination(shown, { initialPageSize: 12, resetKey: term })
  const selected = agents?.find((agent) => agent.id === selectedId) ?? null

  const create = useMutation({
    mutationFn: () => api.createAgent(newName.trim(), newDescription.trim()),
    onSuccess: (agent) => {
      setNewName('')
      setNewDescription('')
      setNewOpen(false)
      setSelectedId(agent.id)
      invalidate()
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })

  if (error) return <p className="p-8 text-app-label text-destructive">{errorMessage(error)}</p>
  if (!agents) return <PageLoader label="Loading analysts" />

  if (selected) {
    return (
      <div>
        <DetailHeader title={<span className="text-app-label text-muted-foreground">Analysts</span>} onBack={() => setSelectedId(null)} />
        <AnalystDetail key={selected.id} agent={selected} canDelete={agents.length > 1} onDeleted={() => setSelectedId(null)} />
      </div>
    )
  }

  const tablesLabel = (agent: Agent) => (agent.assignedTables.length === 0 ? 'All tables' : `${agent.assignedTables.length} ${agent.assignedTables.length === 1 ? 'table' : 'tables'}`)

  return (
    <PageContainer width="full">
      <PageHeader
        title="Analysts"
        description="Each analyst keeps its own rules, examples and memory. Questions go to the one whose focus fits best."
        actions={
          <Button onClick={() => setNewOpen(true)}>
            <Plus strokeWidth={iconStroke} /> New analyst
          </Button>
        }
      />
      {agents.length === 0 ? (
        <EmptyState
          icon={Brain}
          title="No analysts yet"
          description="An analyst keeps its own rules, examples and memory."
          action={<Button onClick={() => setNewOpen(true)}>New analyst</Button>}
        />
      ) : (
        <section className="space-y-4">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <SearchInput value={search} onChange={setSearch} placeholder="Search analysts" className="w-full max-w-sm" />
            <ViewToggle value={view} onChange={setView} />
          </div>
          {shown.length === 0 ? (
            <p className="text-app-label text-muted-foreground">No analysts match “{search}”.</p>
          ) : view === 'cards' ? (
            <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
              {pg.pageItems.map((agent) => (
                <button
                  key={agent.id}
                  type="button"
                  onClick={() => setSelectedId(agent.id)}
                  className="flex flex-col gap-3 rounded-xl border border-border bg-surface p-4 text-left transition-colors duration-fast hover:border-foreground/25 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40"
                >
                  <span className="flex items-center gap-3">
                    <Avatar name={agent.name} />
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-app-label font-semibold text-foreground">{agent.name}</span>
                      <span className="block text-app-caption text-muted-foreground">Created {relativeTime(agent.createdAt)}</span>
                    </span>
                  </span>
                  <span className="line-clamp-2 min-h-[2.5em] text-app-caption text-muted-foreground">{agent.description || 'No description yet.'}</span>
                  <span className="mt-auto flex items-center gap-1.5">
                    <Badge variant="secondary">{tablesLabel(agent)}</Badge>
                  </span>
                </button>
              ))}
            </div>
          ) : (
            <div className="overflow-hidden rounded-xl border border-border bg-surface">
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Analyst</TableHead>
                    <TableHead>Focus</TableHead>
                    <TableHead>Tables</TableHead>
                    <TableHead>Created</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {pg.pageItems.map((agent) => (
                    <TableRow key={agent.id} className="cursor-pointer" onClick={() => setSelectedId(agent.id)}>
                      <TableCell>
                        <span className="flex items-center gap-2.5">
                          <Avatar name={agent.name} size="sm" />
                          <span className="font-medium text-foreground">{agent.name}</span>
                        </span>
                      </TableCell>
                      <TableCell className="max-w-0 w-[45%]">
                        <span className="block truncate text-muted-foreground">{agent.description || 'No description yet.'}</span>
                      </TableCell>
                      <TableCell className="text-muted-foreground">{tablesLabel(agent)}</TableCell>
                      <TableCell className="text-muted-foreground">{relativeTime(agent.createdAt)}</TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </div>
          )}
          <PaginationBar alwaysShow page={pg.page} pageSize={pg.pageSize} total={pg.total} onPageChange={pg.setPage} onPageSizeChange={pg.setPageSize} />
        </section>
      )}
      <FormSheet
        open={newOpen}
        onOpenChange={setNewOpen}
        title="New analyst"
        description="Give it a focus, like Finance or Operations. You can add rules and examples next."
        submitLabel="Create analyst"
        onSubmit={() => create.mutate()}
        submitting={create.isPending}
        canSubmit={Boolean(newName.trim())}
      >
        <FormField label="Name" htmlFor="new-agent-name">
          <Input id="new-agent-name" autoFocus value={newName} onChange={(event) => setNewName(event.target.value)} placeholder="Finance" />
        </FormField>
        <FormField label="What it is responsible for" htmlFor="new-agent-description" hint="Questions are routed to the analyst whose focus fits best.">
          <Textarea id="new-agent-description" rows={4} value={newDescription} onChange={(event) => setNewDescription(event.target.value)} placeholder="Revenue, refunds and margins by region" />
        </FormField>
      </FormSheet>
    </PageContainer>
  )
}

function AnalystDetail({ agent, canDelete, onDeleted }: { agent: Agent; canDelete: boolean; onDeleted: () => void }) {
  const navigate = useNavigation((state) => state.navigate)
  const [tab, setTab] = useState('profile')
  const [deleteOpen, setDeleteOpen] = useState(false)
  const { data: rules = [] } = useQuery({ queryKey: queryKeys.rules(agent.id), queryFn: () => api.rules(agent.id) })
  const { data: examples = [] } = useQuery({ queryKey: queryKeys.examples(agent.id), queryFn: () => api.examples(agent.id) })
  const { data: memories = [] } = useQuery({ queryKey: queryKeys.memories(agent.id), queryFn: () => api.memories(agent.id) })
  const { data: documents = [] } = useQuery({ queryKey: queryKeys.knowledge(agent.id), queryFn: () => api.knowledge(agent.id) })
  const proposed = memories.filter((memory) => memory.status === 'proposed').length
  const active = memories.filter((memory) => memory.status === 'active').length
  const remove = useMutation({
    mutationFn: () => api.deleteAgent(agent.id),
    onSuccess: () => {
      invalidate()
      onDeleted()
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })

  return (
    <div className="mx-auto max-w-5xl space-y-6 px-8 py-8">
      <header className="flex flex-wrap items-start gap-4">
        <Avatar name={agent.name} size="lg" />
        <div className="min-w-0 flex-1">
          <h1 className="font-serif text-app-page text-foreground">{agent.name}</h1>
          <p className="mt-1 text-app-label text-muted-foreground">{agent.description || 'No description yet. Add one so questions are routed here.'}</p>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          <Button variant="outline" onClick={() => navigate('home')}>
            <MessageSquarePlus strokeWidth={iconStroke} /> Ask a question
          </Button>
          {canDelete && (
            <MoreMenu>
              <DropdownMenuItem className="text-destructive focus:text-destructive" onSelect={() => setDeleteOpen(true)}>
                <Trash2 size={14} strokeWidth={iconStroke} /> Delete analyst
              </DropdownMenuItem>
            </MoreMenu>
          )}
        </div>
      </header>

      <StatRow
        stats={[
          { label: 'Rules', value: rules.length, icon: ListChecks, onClick: () => setTab('rules') },
          { label: 'Examples', value: examples.length, icon: BookOpen, onClick: () => setTab('examples') },
          { label: 'Memories', value: active, hint: proposed ? `${proposed} waiting for review` : undefined, icon: Brain, onClick: () => setTab('memory') },
          { label: 'Documents', value: documents.length, icon: FileText, onClick: () => setTab('knowledge') },
        ]}
      />

      {proposed > 0 && (
        <Banner
          icon={Brain}
          tone="warning"
          title={`${proposed} ${proposed === 1 ? 'memory needs' : 'memories need'} your review`}
          description="The analyst noticed standing instructions in chat and is asking before it keeps them."
          actions={
            <Button size="sm" onClick={() => setTab('memory')}>
              Review
            </Button>
          }
        />
      )}

      <Tabs value={tab} onValueChange={setTab}>
        <TabsList variant="underline">
          <TabsTrigger variant="underline" value="profile">
            Profile
          </TabsTrigger>
          <TabsTrigger variant="underline" value="rules">
            Rules <TabCount n={rules.length} />
          </TabsTrigger>
          <TabsTrigger variant="underline" value="examples">
            Examples <TabCount n={examples.length} />
          </TabsTrigger>
          <TabsTrigger variant="underline" value="memory">
            Memory <TabCount n={memories.length} />
          </TabsTrigger>
          <TabsTrigger variant="underline" value="knowledge">
            Knowledge <TabCount n={documents.length} />
          </TabsTrigger>
          <TabsTrigger variant="underline" value="tools">
            Tools
          </TabsTrigger>
        </TabsList>
        <TabsContent value="profile" forceMount className="data-[state=inactive]:hidden">
          <Profile agent={agent} />
        </TabsContent>
        <TabsContent value="rules" forceMount className="data-[state=inactive]:hidden">
          <Rules agentId={agent.id} />
        </TabsContent>
        <TabsContent value="examples" forceMount className="data-[state=inactive]:hidden">
          <Examples agentId={agent.id} />
        </TabsContent>
        <TabsContent value="memory" forceMount className="data-[state=inactive]:hidden">
          <Memories agentId={agent.id} />
        </TabsContent>
        <TabsContent value="knowledge">
          <Knowledge agentId={agent.id} />
        </TabsContent>
        <TabsContent value="tools">
          <McpTools agentId={agent.id} />
        </TabsContent>
      </Tabs>
      <ConfirmDialog
        open={deleteOpen}
        onOpenChange={setDeleteOpen}
        title={`Delete “${agent.name}”?`}
        description="Its rules, examples, memory and knowledge are removed too."
        onConfirm={() => remove.mutate()}
      />
    </div>
  )
}

function TabCount({ n }: { n: number }) {
  if (!n) return null
  return <span className="ml-1.5 rounded-full bg-muted px-1.5 text-app-caption tabular-nums text-muted-foreground">{n}</span>
}

function McpTools({ agentId }: { agentId: string }) {
  const { data: servers = [] } = useQuery({ queryKey: queryKeys.mcpServers, queryFn: api.mcpServers })
  const { data: rules = [] } = useQuery({ queryKey: queryKeys.mcpRules(agentId), queryFn: () => api.mcpRules(agentId) })
  const [serverId, setServerId] = useState<string | null>(null)
  const selected = serverId ?? servers[0]?.id
  const { data: tools = [] } = useQuery({
    queryKey: queryKeys.mcpTools(selected ?? ''),
    queryFn: () => api.mcpTools(selected!),
    enabled: Boolean(selected),
  })
  const toolPg = usePagination(tools, { initialPageSize: 10, resetKey: `${agentId}:${selected}` })
  const resetRule = useMutation({
    mutationFn: (toolName: string) => api.mcpDeleteRule(agentId, selected!, toolName),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.mcpRules(agentId) }),
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const setRule = useMutation({
    mutationFn: (draft: { toolName: string; allow: boolean }) =>
      api.mcpSetRule({ agentId, serverId: selected!, toolName: draft.toolName, allow: draft.allow }),
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.mcpRules(agentId) }),
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  if (servers.length === 0)
    return (
      <div className="pt-2">
        <EmptyState icon={Wrench} title="No tool servers connected" description="Connect an MCP server in Settings → Integrations to give this analyst extra tools." />
      </div>
    )
  return (
    <section className="space-y-4">
      <TabHeader title="Tools" description="Read-only tools are allowed unless you turn them off. Tools that change things stay off until you allow them." />
      {servers.length > 1 && (
        <SegmentedControl value={selected ?? ''} options={servers.map((server) => ({ value: server.id, label: server.name }))} onChange={setServerId} />
      )}
      {tools.length === 0 ? (
        <EmptyState icon={Wrench} title="No tools" description="This server exposes no tools." />
      ) : (
        <div className="divide-y divide-border rounded-xl border border-border bg-surface">
          {toolPg.pageItems.map((tool) => {
            const rule = rules.find((item) => item.serverId === selected && item.toolName === tool.name)
            const allowed = rule ? rule.allow : tool.readOnly
            return (
              <div key={tool.name} className="flex items-center gap-3 px-4 py-3">
                <IconTile icon={Wrench} size="sm" tone={allowed ? 'success' : 'muted'} />
                <div className="min-w-0 flex-1">
                  <p className="flex items-center gap-2 font-mono text-app-caption text-foreground">
                    {tool.name}
                    <Badge variant={rule ? 'brand' : 'outline'}>{rule ? 'Custom' : 'Default'}</Badge>
                  </p>
                  <p className="mt-0.5 text-app-caption text-muted-foreground">{tool.description || (tool.readOnly ? 'Read-only' : 'Can change things')}</p>
                </div>
                {rule && (
                  <Button size="sm" variant="ghost" disabled={resetRule.isPending} onClick={() => resetRule.mutate(tool.name)}>
                    Reset
                  </Button>
                )}
                <Switch checked={allowed} onCheckedChange={(next) => setRule.mutate({ toolName: tool.name, allow: next })} aria-label={`Allow ${tool.name}`} />
              </div>
            )
          })}
        </div>
      )}
      <PaginationBar page={toolPg.page} pageSize={toolPg.pageSize} total={toolPg.total} onPageChange={toolPg.setPage} onPageSizeChange={toolPg.setPageSize} className="pt-1" />
    </section>
  )
}

function Profile({ agent }: { agent: Agent }) {
  const { data: tables = [] } = useQuery({ queryKey: queryKeys.tables, queryFn: api.tables })
  const { data: catalog } = useQuery({ queryKey: queryKeys.models, queryFn: api.modelCatalog })
  const [name, setName] = useState(agent.name)
  const [description, setDescription] = useState(agent.description)
  const [overrides, setOverrides] = useState<TaskRouting>(agent.modelOverrides)
  const [assigned, setAssigned] = useState<string[]>(agent.assignedTables)
  const [tableSearch, setTableSearch] = useState('')

  const dirty =
    name !== agent.name ||
    description !== agent.description ||
    JSON.stringify(overrides) !== JSON.stringify(agent.modelOverrides) ||
    [...assigned].sort().join() !== [...agent.assignedTables].sort().join()
  const discard = () => {
    setName(agent.name)
    setDescription(agent.description)
    setOverrides(agent.modelOverrides)
    setAssigned(agent.assignedTables)
  }

  const save = useMutation({
    mutationFn: () => api.updateAgent({ id: agent.id, name: name.trim(), description, modelOverrides: overrides, assignedTables: assigned }),
    onSuccess: () => {
      toast.success('Analyst saved')
      invalidate()
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })

  const toggleTable = (relation: string) =>
    setAssigned((current) => (current.includes(relation) ? current.filter((item) => item !== relation) : [...current, relation]))
  const term = tableSearch.trim().toLowerCase()
  const visibleTables = tables.filter((table) => !term || table.relation.toLowerCase().includes(term))
  const overridden = (['sql', 'reasoning', 'narrative'] as const).filter((task) => overrides[task]).length

  return (
    <section className="space-y-5 pt-2">
      <div className="grid gap-5 lg:grid-cols-2">
        <Panel title="About" description="Used to route questions to this analyst.">
          <div className="space-y-4">
            <FormField label="Name" htmlFor="agent-name">
              <Input id="agent-name" value={name} onChange={(event) => setName(event.target.value)} />
            </FormField>
            <FormField label="Responsible for" htmlFor="agent-description">
              <Textarea
                id="agent-description"
                value={description}
                onChange={(event) => setDescription(event.target.value)}
                rows={4}
                placeholder="Revenue, refunds and margins by region"
              />
            </FormField>
          </div>
        </Panel>
        <Panel
          title="Tables it can use"
          description={assigned.length === 0 ? 'All tables, because none are picked.' : `${assigned.length} of ${tables.length} picked.`}
          actions={
            assigned.length > 0 ? (
              <Button variant="ghost" size="sm" onClick={() => setAssigned([])}>
                Use all
              </Button>
            ) : undefined
          }
        >
          {tables.length === 0 ? (
            <EmptyState icon={ScrollText} title="No tables connected yet" />
          ) : (
            <div className="space-y-2">
              {tables.length > 6 && <SearchInput value={tableSearch} onChange={setTableSearch} placeholder="Search tables" />}
              <div className="max-h-64 divide-y divide-border overflow-y-auto rounded-lg border border-border">
                {visibleTables.map((table) => (
                  <label key={table.id} className="flex cursor-pointer items-center gap-3 px-3 py-2 transition-colors duration-fast hover:bg-accent/40">
                    <Checkbox checked={assigned.includes(table.relation)} onCheckedChange={() => toggleTable(table.relation)} />
                    <span className="min-w-0 flex-1 truncate font-mono text-app-caption text-foreground">{table.relation}</span>
                    <span className="shrink-0 text-app-caption tabular-nums text-muted-foreground">
                      {table.row_count === null ? '' : `${table.row_count.toLocaleString()} rows`}
                    </span>
                  </label>
                ))}
              </div>
            </div>
          )}
        </Panel>
      </div>
      <Panel title="Models" description={overridden ? `${overridden} of 3 tasks use their own model.` : 'Every task uses the workspace default model.'}>
        <div className="grid gap-4 sm:grid-cols-3">
          {(['sql', 'reasoning', 'narrative'] as const).map((task) => (
            <FormField key={task} label={TASK_LABEL[task]}>
              <Select
                value={overrides[task] || DEFAULT}
                onValueChange={(value) => setOverrides((current) => ({ ...current, [task]: value === DEFAULT ? null : value }))}
              >
                <SelectTrigger aria-label={`${TASK_LABEL[task]} model`}>
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem value={DEFAULT}>Workspace default</SelectItem>
                  {catalog?.models.map((model) => (
                    <SelectItem key={model.id} value={model.id}>
                      {model.name}
                      {model.installed ? '' : ' (not downloaded)'}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </FormField>
          ))}
        </div>
      </Panel>
      <SaveBar dirty={dirty && Boolean(name.trim())} saving={save.isPending} onSave={() => save.mutate()} onDiscard={discard} />
    </section>
  )
}

function TabHeader({ title, description, action, search, onSearch, searchLabel }: { title: string; description: string; action?: ReactNode; search?: string; onSearch?: (value: string) => void; searchLabel?: string }) {
  return (
    <div className="space-y-3 pt-2">
      <SectionHeader title={title} description={description} actions={action} />
      {onSearch && <SearchInput value={search ?? ''} onChange={onSearch} placeholder={searchLabel ?? 'Search'} className="max-w-sm" />}
    </div>
  )
}

function RuleDialog({ agentId, rule, open, onOpenChange }: { agentId: string; rule: Rule | null; open: boolean; onOpenChange: (open: boolean) => void }) {
  const { data: tables = [] } = useQuery({ queryKey: queryKeys.tables, queryFn: api.tables })
  const [title, setTitle] = useState(rule?.title ?? '')
  const [body, setBody] = useState(rule?.body ?? '')
  const [tableName, setTableName] = useState(rule?.tableName ?? '')
  const save = useMutation({
    mutationFn: () => api.saveRule({ id: rule?.id, agentId, title, body, tableName: tableName || null }),
    onSuccess: () => {
      invalidate(agentId)
      onOpenChange(false)
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  return (
    <FormSheet
      open={open}
      onOpenChange={onOpenChange}
      title={rule ? 'Edit rule' : 'New rule'}
      description="A definition or gotcha the analyst follows. Every save keeps the previous wording."
      submitLabel={rule ? 'Save rule' : 'Add rule'}
      onSubmit={() => save.mutate()}
      submitting={save.isPending}
      canSubmit={Boolean(body.trim())}
    >
      <FormField label="Title" htmlFor="rule-title" hint="Optional">
        <Input id="rule-title" value={title} onChange={(event) => setTitle(event.target.value)} placeholder="Revenue" />
      </FormField>
      <FormField label="Rule" htmlFor="rule-body">
        <Textarea id="rule-body" autoFocus value={body} onChange={(event) => setBody(event.target.value)} rows={6} placeholder="Revenue is net of refunds." />
      </FormField>
      <FormField label="Applies to">
        <Select value={tableName || ALL_TABLES} onValueChange={(value) => setTableName(value === ALL_TABLES ? '' : value)}>
          <SelectTrigger aria-label="Table this rule applies to">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={ALL_TABLES}>All tables</SelectItem>
            {tables.map((table) => (
              <SelectItem key={table.id} value={table.relation}>
                {table.relation}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </FormField>
    </FormSheet>
  )
}

function Rules({ agentId }: { agentId: string }) {
  const { data: rules = [] } = useQuery({ queryKey: queryKeys.rules(agentId), queryFn: () => api.rules(agentId) })
  const [search, setSearch] = useState('')
  const [dialog, setDialog] = useState<{ rule: Rule | null } | null>(null)
  const [history, setHistory] = useState<Rule | null>(null)
  const [removing, setRemoving] = useState<Rule | null>(null)
  const term = search.trim().toLowerCase()
  const shown = rules.filter((rule) => !term || `${rule.title} ${rule.body} ${rule.tableName ?? ''}`.toLowerCase().includes(term))
  const rulePg = usePagination(shown, { initialPageSize: 10, resetKey: `${agentId}|${term}` })
  const remove = useMutation({
    mutationFn: api.deleteRule,
    onSuccess: () => invalidate(agentId),
    onError: (failure) => toast.error(errorMessage(failure)),
  })

  return (
    <section className="space-y-4">
      <TabHeader
        title="Rules"
        description="Definitions and gotchas. Every save keeps the previous wording, and you can roll back."
        action={
          <Button size="sm" onClick={() => setDialog({ rule: null })}>
            <Plus strokeWidth={iconStroke} /> New rule
          </Button>
        }
        search={search}
        onSearch={rules.length > 5 ? setSearch : undefined}
        searchLabel="Search rules"
      />
      {rules.length === 0 ? (
        <EmptyState icon={ListChecks} title="No rules yet" description="Teach the analyst a definition, like how revenue is counted." action={<Button onClick={() => setDialog({ rule: null })}>New rule</Button>} />
      ) : (
        <div className="divide-y divide-border rounded-xl border border-border bg-surface">
          {rulePg.pageItems.map((rule) => (
            <div key={rule.id} className="group flex items-start gap-3 px-4 py-3">
              <IconTile icon={ListChecks} size="sm" tone="muted" />
              <button type="button" onClick={() => setDialog({ rule })} className="min-w-0 flex-1 rounded text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40">
                <span className="flex items-center gap-2">
                  <span className="text-app-label font-medium text-foreground">{rule.title || 'Untitled rule'}</span>
                  {rule.tableName && <Badge variant="outline" className="font-mono">{rule.tableName}</Badge>}
                </span>
                <span className="mt-0.5 line-clamp-2 block whitespace-pre-wrap text-app-caption text-muted-foreground">{rule.body}</span>
              </button>
              <RowMenu label={`Actions for ${rule.title || 'rule'}`}>
                <DropdownMenuItem onSelect={() => setDialog({ rule })}>Edit</DropdownMenuItem>
                <DropdownMenuItem onSelect={() => setHistory(rule)}>
                  <History size={14} strokeWidth={iconStroke} /> History
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem className="text-destructive focus:text-destructive" onSelect={() => setRemoving(rule)}>
                  Delete
                </DropdownMenuItem>
              </RowMenu>
            </div>
          ))}
        </div>
      )}
      <PaginationBar page={rulePg.page} pageSize={rulePg.pageSize} total={rulePg.total} onPageChange={rulePg.setPage} onPageSizeChange={rulePg.setPageSize} className="pt-1" />
      {dialog && <RuleDialog key={dialog.rule?.id ?? 'new'} agentId={agentId} rule={dialog.rule} open onOpenChange={(open) => !open && setDialog(null)} />}
      <Sheet open={history !== null} onOpenChange={(open) => !open && setHistory(null)}>
        <SheetContent>
          {history && (
            <>
              <SheetHeader title={history.title || 'Untitled rule'} description="Earlier wordings of this rule. Restore any of them." />
              <SheetBody>
                <RuleHistory rule={history} />
              </SheetBody>
            </>
          )}
        </SheetContent>
      </Sheet>
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(open) => !open && setRemoving(null)}
        title="Delete this rule?"
        description="The rule and its history are removed."
        onConfirm={() => removing && remove.mutate(removing.id)}
      />
    </section>
  )
}

function RuleHistory({ rule }: { rule: Rule }) {
  const { data: versions = [], isPending } = useQuery({ queryKey: queryKeys.ruleVersions(rule.id), queryFn: () => api.ruleVersions(rule.id) })
  const rollback = useMutation({
    mutationFn: (versionId: string) => api.rollbackRule(rule.id, versionId),
    onSuccess: () => {
      toast.success('Rule restored')
      invalidate(rule.agentId)
      void queryClient.invalidateQueries({ queryKey: queryKeys.ruleVersions(rule.id) })
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  if (isPending) return <PageLoader label="Loading history" />
  if (versions.length === 0) return <EmptyState icon={History} title="No earlier versions" description="Edits are kept here from now on." />
  return (
    <ul className="space-y-3">
      {versions.map((version) => (
        <VersionRow key={version.id} version={version} rule={rule} onRollback={() => rollback.mutate(version.id)} busy={rollback.isPending} />
      ))}
    </ul>
  )
}

function VersionRow({ version, rule, onRollback, busy }: { version: RuleVersion; rule: Rule; onRollback: () => void; busy: boolean }) {
  const same = version.snapshot.title === rule.title && version.snapshot.body === rule.body && version.snapshot.tableName === rule.tableName
  const removed = version.snapshot.body.split('\n').filter((line) => !rule.body.split('\n').includes(line))
  const added = rule.body.split('\n').filter((line) => !version.snapshot.body.split('\n').includes(line))
  return (
    <li className="rounded-xl border border-border bg-surface px-4 py-3">
      <div className="flex items-center justify-between gap-2">
        <span className="text-app-caption text-muted-foreground">{relativeTime(version.createdAt)}</span>
        {same ? (
          <Badge variant="success">Current</Badge>
        ) : (
          <Button type="button" variant="outline" size="sm" onClick={onRollback} disabled={busy}>
            Restore
          </Button>
        )}
      </div>
      {!same && (
        <pre className="mt-2 whitespace-pre-wrap rounded-lg bg-muted/40 p-2 font-mono text-app-caption">
          {removed.map((line, index) => (
            <div key={`removed-${index}`} className="text-destructive">
              - {line}
            </div>
          ))}
          {added.map((line, index) => (
            <div key={`added-${index}`} className="text-success">
              + {line}
            </div>
          ))}
        </pre>
      )}
    </li>
  )
}

function Examples({ agentId }: { agentId: string }) {
  const { data: examples = [] } = useQuery({ queryKey: queryKeys.examples(agentId), queryFn: () => api.examples(agentId) })
  const [search, setSearch] = useState('')
  const [open, setOpen] = useState(false)
  const [question, setQuestion] = useState('')
  const [sql, setSql] = useState('')
  const [removing, setRemoving] = useState<string | null>(null)
  const term = search.trim().toLowerCase()
  const shown = examples.filter((example) => !term || `${example.question} ${example.sql}`.toLowerCase().includes(term))
  const examplePg = usePagination(shown, { initialPageSize: 8, resetKey: `${agentId}|${term}` })
  const save = useMutation({
    mutationFn: () => api.saveExample(agentId, question, sql),
    onSuccess: () => {
      setQuestion('')
      setSql('')
      setOpen(false)
      invalidate(agentId)
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const remove = useMutation({
    mutationFn: api.deleteExample,
    onSuccess: () => invalidate(agentId),
    onError: (failure) => toast.error(errorMessage(failure)),
  })

  return (
    <section className="space-y-4">
      <TabHeader
        title="Examples"
        description="Reviewed question and SQL pairs. The closest ones are shown to the model."
        action={
          <Button size="sm" onClick={() => setOpen(true)}>
            <Plus strokeWidth={iconStroke} /> New example
          </Button>
        }
        search={search}
        onSearch={examples.length > 4 ? setSearch : undefined}
        searchLabel="Search examples"
      />
      {examples.length === 0 ? (
        <EmptyState icon={BookOpen} title="No examples yet" description="Thumbs up on a good answer saves it here, or add one by hand." action={<Button onClick={() => setOpen(true)}>New example</Button>} />
      ) : (
        <div className="grid gap-3 xl:grid-cols-2">
          {examplePg.pageItems.map((example) => (
            <div key={example.id} className="group flex flex-col rounded-xl border border-border bg-surface">
              <div className="flex items-start gap-2 px-4 pt-3">
                <p className="min-w-0 flex-1 text-app-label font-medium text-foreground">{example.question}</p>
                {example.origin === 'thumbs_up' && <Badge variant="brand">From a liked answer</Badge>}
                <RowMenu label="Example actions">
                  <DropdownMenuItem className="text-destructive focus:text-destructive" onSelect={() => setRemoving(example.id)}>
                    Delete
                  </DropdownMenuItem>
                </RowMenu>
              </div>
              <pre className="m-3 mt-2 max-h-40 overflow-auto rounded-lg bg-muted/50 p-3 font-mono text-app-caption text-foreground/90">{example.sql}</pre>
            </div>
          ))}
        </div>
      )}
      <PaginationBar page={examplePg.page} pageSize={examplePg.pageSize} total={examplePg.total} onPageChange={examplePg.setPage} onPageSizeChange={examplePg.setPageSize} className="pt-1" />
      <FormSheet
        open={open}
        onOpenChange={setOpen}
        width="lg"
        title="New example"
        description="A question and the SQL that answers it correctly. The analyst copies the approach for similar questions."
        submitLabel="Add example"
        onSubmit={() => save.mutate()}
        submitting={save.isPending}
        canSubmit={Boolean(question.trim() && sql.trim())}
      >
        <FormField label="Question" htmlFor="example-question">
          <Input id="example-question" autoFocus value={question} onChange={(event) => setQuestion(event.target.value)} placeholder="What was net revenue last month?" />
        </FormField>
        <FormField label="SQL" htmlFor="example-sql">
          <Textarea id="example-sql" value={sql} onChange={(event) => setSql(event.target.value)} rows={12} className="font-mono text-app-caption" placeholder="SELECT …" />
        </FormField>
      </FormSheet>
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(open) => !open && setRemoving(null)}
        title="Delete this example?"
        description="The question and SQL pair is removed."
        onConfirm={() => removing && remove.mutate(removing)}
      />
    </section>
  )
}

function Memories({ agentId }: { agentId: string }) {
  const { data: memories = [] } = useQuery({ queryKey: queryKeys.memories(agentId), queryFn: () => api.memories(agentId) })
  const [dialog, setDialog] = useState<{ id?: string; body: string } | null>(null)
  const [removing, setRemoving] = useState<string | null>(null)
  const save = useMutation({
    mutationFn: () => api.saveMemory({ id: dialog?.id, agentId, body: dialog?.body ?? '' }),
    onSuccess: () => {
      setDialog(null)
      invalidate(agentId)
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const setStatus = useMutation({
    mutationFn: ({ id, status }: { id: string; status: 'active' | 'archived' | 'proposed' }) => api.setMemoryStatus(id, status),
    onSuccess: () => invalidate(agentId),
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const remove = useMutation({
    mutationFn: api.deleteMemory,
    onSuccess: () => invalidate(agentId),
    onError: (failure) => toast.error(errorMessage(failure)),
  })

  const groups = [
    { status: 'proposed' as const, title: 'Waiting for review', description: 'Suggested from chat. Keep it to apply it to future answers.' },
    { status: 'active' as const, title: 'Active', description: 'Applied to every question this analyst answers.' },
    { status: 'archived' as const, title: 'Archived', description: 'Kept for reference, not applied.' },
  ]

  return (
    <section className="space-y-5">
      <TabHeader
        title="Memory"
        description="Standing instructions, like “exclude test accounts”."
        action={
          <Button size="sm" onClick={() => setDialog({ body: '' })}>
            <Plus strokeWidth={iconStroke} /> New memory
          </Button>
        }
      />
      {memories.length === 0 && (
        <EmptyState icon={Brain} title="No memory yet" description="Say “from now on…” in chat, or add one here." action={<Button onClick={() => setDialog({ body: '' })}>New memory</Button>} />
      )}
      {groups.map((group) => {
        const items = memories.filter((memory) => memory.status === group.status)
        if (items.length === 0) return null
        return (
          <div key={group.status} className="space-y-2">
            <p className="text-app-caption font-medium text-muted-foreground">
              {group.title} · {items.length}
              <span className="ml-2 font-normal">{group.description}</span>
            </p>
            <div className={`divide-y divide-border rounded-xl border bg-surface ${group.status === 'proposed' ? 'border-warning/40' : 'border-border'}`}>
              {items.map((memory) => (
                <div key={memory.id} className="group flex items-center gap-3 px-4 py-3">
                  <span className={`size-2 shrink-0 rounded-full ${memory.status === 'active' ? 'bg-success' : memory.status === 'proposed' ? 'bg-warning' : 'bg-muted-foreground/40'}`} aria-hidden />
                  <p className={`min-w-0 flex-1 text-app-label ${memory.status === 'archived' ? 'text-muted-foreground' : 'text-foreground'}`}>{memory.body}</p>
                  {memory.status === 'proposed' && (
                    <>
                      <Button size="sm" onClick={() => setStatus.mutate({ id: memory.id, status: 'active' })}>
                        Keep
                      </Button>
                      <Button size="sm" variant="ghost" onClick={() => setStatus.mutate({ id: memory.id, status: 'archived' })}>
                        Dismiss
                      </Button>
                    </>
                  )}
                  <RowMenu label="Memory actions">
                    <DropdownMenuItem onSelect={() => setDialog({ id: memory.id, body: memory.body })}>Edit</DropdownMenuItem>
                    {memory.status === 'active' && <DropdownMenuItem onSelect={() => setStatus.mutate({ id: memory.id, status: 'archived' })}>Archive</DropdownMenuItem>}
                    {memory.status === 'archived' && <DropdownMenuItem onSelect={() => setStatus.mutate({ id: memory.id, status: 'active' })}>Use again</DropdownMenuItem>}
                    <DropdownMenuSeparator />
                    <DropdownMenuItem className="text-destructive focus:text-destructive" onSelect={() => setRemoving(memory.id)}>
                      Delete
                    </DropdownMenuItem>
                  </RowMenu>
                </div>
              ))}
            </div>
          </div>
        )
      })}
      <FormSheet
        open={dialog !== null}
        onOpenChange={(open) => !open && setDialog(null)}
        title={dialog?.id ? 'Edit memory' : 'New memory'}
        description="A standing instruction applied to every answer."
        submitLabel={dialog?.id ? 'Save memory' : 'Add memory'}
        onSubmit={() => save.mutate()}
        submitting={save.isPending}
        canSubmit={Boolean(dialog?.body.trim())}
      >
        <FormField label="Instruction" htmlFor="memory-body">
          <Textarea
            id="memory-body"
            autoFocus
            rows={5}
            value={dialog?.body ?? ''}
            onChange={(event) => setDialog((current) => (current ? { ...current, body: event.target.value } : current))}
            placeholder="Exclude test accounts"
          />
        </FormField>
      </FormSheet>
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(open) => !open && setRemoving(null)}
        title="Delete this memory?"
        description="The instruction is removed."
        onConfirm={() => removing && remove.mutate(removing)}
      />
    </section>
  )
}

function Knowledge({ agentId }: { agentId: string }) {
  const { data: files = [] } = useQuery({ queryKey: queryKeys.knowledge(agentId), queryFn: () => api.knowledge(agentId) })
  const [removing, setRemoving] = useState<{ id: string; name: string } | null>(null)
  const add = useMutation({
    mutationFn: async () => {
      const picked = await open({
        multiple: false,
        directory: false,
        filters: [{ name: 'Documents', extensions: ['pdf', 'docx', 'md', 'txt'] }],
      })
      if (typeof picked !== 'string') return null
      return api.addKnowledge(agentId, picked)
    },
    onSuccess: (file) => {
      if (file) toast.success(`Learned ${file.name}`)
      invalidate(agentId)
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const remove = useMutation({
    mutationFn: api.deleteKnowledge,
    onSuccess: () => invalidate(agentId),
    onError: (failure) => toast.error(errorMessage(failure)),
  })

  const addButton = (
    <Button size="sm" loading={add.isPending} onClick={() => add.mutate()}>
      {!add.isPending && <Plus strokeWidth={iconStroke} />} Add a document
    </Button>
  )
  return (
    <section className="space-y-4">
      <TabHeader title="Knowledge" description="PDF, Word, Markdown or text. Excerpts are used when a question is close to them." action={addButton} />
      {files.length === 0 ? (
        <EmptyState icon={FileText} title="No documents yet" description="Add a glossary, a metrics guide or a policy for this analyst to learn from." action={addButton} />
      ) : (
        <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
          {files.map((file) => (
            <div key={file.id} className="group flex items-center gap-3 rounded-xl border border-border bg-surface px-4 py-3">
              <IconTile icon={FileText} tone="muted" />
              <div className="min-w-0 flex-1">
                <p className="truncate text-app-label font-medium text-foreground">{file.name}</p>
                <p className="text-app-caption text-muted-foreground">
                  {file.chunkCount} excerpts · added {relativeTime(file.createdAt)}
                </p>
              </div>
              <RowMenu label={`Actions for ${file.name}`}>
                <DropdownMenuItem className="text-destructive focus:text-destructive" onSelect={() => setRemoving({ id: file.id, name: file.name })}>
                  Remove
                </DropdownMenuItem>
              </RowMenu>
            </div>
          ))}
        </div>
      )}
      <ConfirmDialog
        open={removing !== null}
        onOpenChange={(open) => !open && setRemoving(null)}
        title={`Remove “${removing?.name ?? ''}”?`}
        description="Its excerpts are no longer used for answers."
        confirmLabel="Remove"
        onConfirm={() => removing && remove.mutate(removing.id)}
      />
    </section>
  )
}
