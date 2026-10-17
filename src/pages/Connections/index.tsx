import { useState } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { open } from '@tauri-apps/plugin-dialog'
import { Check, Database, FileSpreadsheet, FolderOpen, Loader2, Plus, Table2 } from 'lucide-react'
import { cn } from '../../lib/utils'
import { useViewMode } from '../../lib/useViewMode'
import { toast } from 'sonner'
import { SampleTable } from '../../components/data/SampleTable'
import { EmptyState, FormField, PageContainer, PageHeader, PageLoader, RowMenu, SearchInput, SectionHeader, SegmentedControl, StatusBadge, ViewToggle } from '../../components/layout/ui'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '../../components/ui/table'
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '../../components/ui/alert-dialog'
import { Alert, AlertDescription } from '../../components/ui/alert'
import { Badge } from '../../components/ui/badge'
import { Button } from '../../components/ui/button'
import { Checkbox } from '../../components/ui/checkbox'
import { DialogDescription, DialogTitle } from '../../components/ui/dialog'
import { Sheet, SheetContent } from '../../components/ui/sheet'
import { DropdownMenuItem, DropdownMenuSeparator } from '../../components/ui/dropdown-menu'
import { Input } from '../../components/ui/input'
import { PaginationBar } from '../../components/ui/pagination'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../../components/ui/select'
import { Switch } from '../../components/ui/switch'
import { Textarea } from '../../components/ui/textarea'
import { Hint } from '../../components/ui/tooltip'
import { api, errorMessage, type AccessMode, type Connection, type DatabaseKind, type DbConfig, type Dependent, type ExploredTable } from '../../lib/api'
import { iconStroke } from '../../lib/icon'
import { invalidateConnectionData, queryKeys } from '../../lib/queryClient'
import { usePagination } from '../../lib/usePagination'
import { pickSources, useAddSources, usePendingSources } from '../../lib/sources'
import { useNavigation } from '../../stores/navigation'
import { relativeTime } from '../../lib/time'
import { IconTile, StatRow } from '../../components/layout/patterns'

type FieldSpec = {
  /** `host`, `port`, `database`, `username`, `password` map to DbConfig; anything else goes to `extra`. */
  key: string
  label: string
  placeholder?: string
  secret?: boolean
  multiline?: boolean
  file?: boolean
  options?: { value: string; label: string }[]
  wide?: boolean
}

type KindSpec = {
  label: string
  group: string
  mode: AccessMode
  port: number
  host?: string
  note: string
  fields: FieldSpec[]
  extra?: Record<string, string>
}

const ROW_CAP: FieldSpec = { key: 'row_cap', label: 'Snapshot row cap per table', placeholder: '1000000' }

const KINDS: Record<DatabaseKind, KindSpec> = {
  postgres: {
    label: 'PostgreSQL',
    group: 'Databases',
    mode: 'live',
    port: 5432,
    host: 'localhost',
    note: 'Read-only. Tables are read live; nothing is copied.',
    fields: [
      { key: 'host', label: 'Host' },
      { key: 'port', label: 'Port' },
      { key: 'username', label: 'Username' },
      { key: 'password', label: 'Password', secret: true },
      { key: 'database', label: 'Database', wide: true },
    ],
  },
  mysql: {
    label: 'MySQL',
    group: 'Databases',
    mode: 'live',
    port: 3306,
    host: 'localhost',
    note: 'Use an account that can only SELECT. Tables are read live; nothing is copied.',
    fields: [
      { key: 'host', label: 'Host' },
      { key: 'port', label: 'Port' },
      { key: 'username', label: 'Username' },
      { key: 'password', label: 'Password', secret: true },
      { key: 'database', label: 'Database', wide: true },
    ],
  },
  sqlite: {
    label: 'SQLite',
    group: 'Databases',
    mode: 'live',
    port: 0,
    note: 'Opened read-only from a file on this computer.',
    fields: [{ key: 'database', label: 'Database file', file: true, wide: true }],
  },
  sqlserver: {
    label: 'SQL Server',
    group: 'Databases',
    mode: 'snapshot',
    port: 1433,
    host: 'localhost',
    note: 'Use an account that can only SELECT. Tables are copied once as a snapshot.',
    fields: [
      { key: 'host', label: 'Host' },
      { key: 'port', label: 'Port' },
      { key: 'username', label: 'Username' },
      { key: 'password', label: 'Password', secret: true },
      { key: 'database', label: 'Database', wide: true },
      {
        key: 'trust_server_certificate',
        label: 'Server certificate',
        options: [
          { value: 'false', label: 'Verify' },
          { value: 'true', label: 'Trust (self-signed)' },
        ],
      },
      ROW_CAP,
    ],
  },
  supabase: {
    label: 'Supabase',
    group: 'Cloud Postgres',
    mode: 'live',
    port: 5432,
    host: 'aws-0-us-east-1.pooler.supabase.com',
    note: 'Needs SSL. Use the pooler host and a user like postgres.<project-ref>; a read-only role is best.',
    fields: [
      { key: 'host', label: 'Pooler host' },
      { key: 'port', label: 'Port' },
      { key: 'username', label: 'Username', placeholder: 'postgres.<project-ref>' },
      { key: 'password', label: 'Password', secret: true },
      { key: 'database', label: 'Database', wide: true, placeholder: 'postgres' },
    ],
  },
  redshift: {
    label: 'Amazon Redshift',
    group: 'Cloud Postgres',
    mode: 'snapshot',
    port: 5439,
    note: 'Read-only. Tables are copied once as a snapshot.',
    fields: [
      { key: 'host', label: 'Cluster endpoint' },
      { key: 'port', label: 'Port' },
      { key: 'username', label: 'Username' },
      { key: 'password', label: 'Password', secret: true },
      { key: 'database', label: 'Database', wide: true },
      ROW_CAP,
    ],
  },
  motherduck: {
    label: 'MotherDuck',
    group: 'Warehouses',
    mode: 'live',
    port: 0,
    note: 'Read-only, with a read-scaling token. The first connection downloads a small extension, so it needs internet once.',
    fields: [
      { key: 'database', label: 'Database', wide: true },
      { key: 'password', label: 'Token', secret: true, wide: true },
    ],
  },
  snowflake: {
    label: 'Snowflake',
    group: 'Warehouses',
    mode: 'snapshot',
    port: 0,
    note: 'Use a role that can only SELECT. Tables are copied once as a snapshot.',
    extra: { auth: 'pat' },
    fields: [
      { key: 'host', label: 'Account', placeholder: 'xy12345.us-east-1' },
      { key: 'username', label: 'User' },
      { key: 'database', label: 'Database' },
      { key: 'schema', label: 'Default schema (optional)' },
      { key: 'warehouse', label: 'Warehouse' },
      { key: 'role', label: 'Role (optional)' },
      {
        key: 'auth',
        label: 'Authentication',
        options: [
          { value: 'pat', label: 'Programmatic access token' },
          { value: 'keypair', label: 'Key pair (JWT)' },
        ],
      },
      { key: 'password', label: 'Token or unencrypted private key (PEM)', secret: true, multiline: true, wide: true },
      ROW_CAP,
    ],
  },
  bigquery: {
    label: 'BigQuery',
    group: 'Warehouses',
    mode: 'snapshot',
    port: 0,
    note: 'Use a service account with read-only access. Tables are copied once as a snapshot.',
    fields: [
      { key: 'database', label: 'Project id' },
      { key: 'dataset', label: 'Dataset' },
      { key: 'location', label: 'Location (optional)', placeholder: 'US' },
      { key: 'password', label: 'Service-account JSON key', secret: true, multiline: true, wide: true },
      ROW_CAP,
    ],
  },
  databricks: {
    label: 'Databricks',
    group: 'Warehouses',
    mode: 'snapshot',
    port: 0,
    note: 'Use a personal access token. Tables are copied once as a snapshot.',
    fields: [
      { key: 'host', label: 'Workspace host', placeholder: 'dbc-1234.cloud.databricks.com' },
      { key: 'http_path', label: 'HTTP path', placeholder: '/sql/1.0/warehouses/<id>' },
      { key: 'database', label: 'Catalog', placeholder: 'main' },
      { key: 'schema', label: 'Schema', placeholder: 'default' },
      { key: 'password', label: 'Personal access token', secret: true, wide: true },
      ROW_CAP,
    ],
  },
  clickhouse: {
    label: 'ClickHouse',
    group: 'Warehouses',
    mode: 'snapshot',
    port: 8123,
    host: 'localhost',
    note: 'Read-only. Tables are copied once as a snapshot.',
    fields: [
      { key: 'host', label: 'Host' },
      { key: 'port', label: 'HTTP port' },
      { key: 'username', label: 'Username', placeholder: 'default' },
      { key: 'password', label: 'Password', secret: true },
      { key: 'database', label: 'Database', placeholder: 'default' },
      {
        key: 'secure',
        label: 'Protocol',
        options: [
          { value: 'false', label: 'HTTP' },
          { value: 'true', label: 'HTTPS' },
        ],
      },
      ROW_CAP,
    ],
  },
}

const GROUPS = ['Databases', 'Cloud Postgres', 'Warehouses']

function emptyConfig(kind: DatabaseKind): DbConfig {
  const spec = KINDS[kind]
  return {
    name: '',
    kind,
    host: spec.host ?? '',
    port: spec.port,
    database: '',
    username: '',
    password: '',
    extra: { ...spec.extra },
  }
}

function snapshotLabel(connection: Connection): string | null {
  if (connection.mode !== 'snapshot') return null
  if (!connection.snapshotAt) return 'Snapshot · not imported yet'
  const when = new Date(connection.snapshotAt)
  const text = Number.isNaN(when.getTime()) ? connection.snapshotAt : when.toLocaleString()
  return `Snapshot as of ${text}${connection.snapshotTruncated ? ` · capped at ${connection.rowCap?.toLocaleString() ?? ''} rows` : ''}`
}

function useConnectionAction() {
  return useMutation({
    mutationFn: (run: () => Promise<unknown>) => run(),
    onSettled: () => invalidateConnectionData(),
    onError: (failure) => toast.error(errorMessage(failure)),
  })
}


function PendingSources({ paths }: { paths: string[] }) {
  return (
    <div role="status" aria-live="polite" className="space-y-1.5 rounded-xl border border-border bg-surface px-4 py-3">
      <p className="flex items-center gap-2 text-app-label text-foreground">
        <Loader2 size={15} strokeWidth={iconStroke} className="animate-spin text-brand" />
        Reading {paths.length === 1 ? '1 source' : `${paths.length} sources`} and profiling columns…
      </p>
      <ul className="space-y-0.5 pl-6">
        {paths.map((path) => (
          <li key={path}>
            <Hint text={path}>
              <span className="block truncate font-mono text-app-caption text-muted-foreground">{path}</span>
            </Hint>
          </li>
        ))}
      </ul>
    </div>
  )
}

function isDatabase(connection: Connection) {
  return connection.kind !== 'file' && connection.kind !== 'folder'
}

function SourceRow({ connection, onExplore, layout = 'card' }: { connection: Connection; onExplore: (connection: Connection) => void; layout?: 'card' | 'row' }) {
  const action = useConnectionAction()
  const openSchemaFor = useNavigation((state) => state.openSchemaFor)
  const [removeOpen, setRemoveOpen] = useState(false)
  const [dependents, setDependents] = useState<Dependent[] | null>(null)
  const database = isDatabase(connection)
  const canRefresh = !database || (connection.mode === 'snapshot' && connection.tableCount > 0)
  const blocked = (dependents ?? []).length > 0

  const refresh = () =>
    action.mutate(async () => {
      const report = await api.refreshConnection(connection.id)
      for (const message of report.errors) toast.error(message)
    })
  const remove = () => action.mutate(() => api.removeConnection(connection.id))
  const startRemove = async () => {
    if (connection.tableCount === 0) return remove()
    try {
      setDependents(await api.connectionDependents(connection.id))
      setRemoveOpen(true)
    } catch (failure) {
      toast.error(errorMessage(failure))
    }
  }

  const tables = `${connection.tableCount} ${connection.tableCount === 1 ? 'table' : 'tables'}`
  const icon = connection.kind === 'folder' ? FolderOpen : connection.kind === 'file' ? FileSpreadsheet : Database
  const menu = (
    <RowMenu label={`Actions for ${connection.name}`}>
      {connection.tableCount > 0 && <DropdownMenuItem onSelect={() => openSchemaFor(connection.id)}>View tables</DropdownMenuItem>}
      {database && <DropdownMenuItem onSelect={() => onExplore(connection)}>Explore</DropdownMenuItem>}
      {canRefresh && <DropdownMenuItem onSelect={refresh}>{database ? 'Refresh snapshot' : 'Refresh'}</DropdownMenuItem>}
      <DropdownMenuSeparator />
      <DropdownMenuItem className="text-destructive focus:text-destructive" onSelect={() => void startRemove()}>
        Remove
      </DropdownMenuItem>
    </RowMenu>
  )
  const typeBadge = database ? (
    <StatusBadge variant={connection.mode === 'live' ? 'success' : 'warning'}>{connection.mode === 'live' ? 'Live' : 'Snapshot'}</StatusBadge>
  ) : (
    <StatusBadge variant="neutral">{connection.kind === 'folder' ? 'Watched folder' : 'File'}</StatusBadge>
  )
  const removeDialog = (
      <AlertDialog open={removeOpen} onOpenChange={setRemoveOpen}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{blocked ? `“${connection.name}” is in use` : `Remove “${connection.name}”?`}</AlertDialogTitle>
          <AlertDialogDescription>
            {blocked
              ? `Used by ${dependents?.length} ${dependents?.length === 1 ? 'item' : 'items'}. Remove or retarget those first.`
              : 'Its tables, descriptions and detected relationships are removed from this workspace. The source itself is not touched.'}
          </AlertDialogDescription>
        </AlertDialogHeader>
        {blocked && (
          <ul className="max-h-48 space-y-1 overflow-y-auto text-app-label text-foreground">
            {dependents?.map((item) => (
              <li key={`${item.kind}-${item.id}`}>{item.name}</li>
            ))}
          </ul>
        )}
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          {!blocked && (
            <AlertDialogAction onClick={remove} className="bg-destructive text-destructive-foreground hover:bg-destructive/90">
              Remove
            </AlertDialogAction>
          )}
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )

  if (layout === 'row') {
    return (
      <TableRow className="group">
        <TableCell className="max-w-0 w-[45%]">
          <button
            type="button"
            disabled={connection.tableCount === 0}
            onClick={() => openSchemaFor(connection.id)}
            className="flex w-full min-w-0 items-center gap-3 rounded text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40 disabled:cursor-default"
          >
            <IconTile icon={icon} size="sm" tone={database ? 'brand' : 'muted'} />
            <span className="min-w-0">
              <span className="block truncate text-app-label font-medium text-foreground">{connection.name}</span>
              <span className="block truncate font-mono text-app-caption text-muted-foreground">{connection.detail}</span>
            </span>
          </button>
        </TableCell>
        <TableCell>
          <div className="flex items-center gap-1.5">
            {typeBadge}
            {connection.sourceMissing && <StatusBadge variant="warning">Not found</StatusBadge>}
          </div>
        </TableCell>
        <TableCell className="tabular-nums">{connection.tableCount}</TableCell>
        <TableCell className="text-muted-foreground">{snapshotLabel(connection) ?? `Added ${relativeTime(connection.createdAt)}`}</TableCell>
        <TableCell className="w-12 text-right">
          <div className="flex items-center justify-end gap-1">
            {action.isPending && <Loader2 size={15} strokeWidth={iconStroke} className="animate-spin text-muted-foreground" />}
            {menu}
          </div>
          {removeDialog}
        </TableCell>
      </TableRow>
    )
  }

  return (
    <div className="group flex flex-col rounded-xl border border-border bg-surface transition-colors duration-fast hover:border-foreground/25">
      <div className="flex items-start gap-3 px-4 pt-4">
        <IconTile icon={icon} tone={database ? 'brand' : 'muted'} />
        <button
          type="button"
          disabled={connection.tableCount === 0}
          onClick={() => openSchemaFor(connection.id)}
          className="min-w-0 flex-1 rounded text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40 disabled:cursor-default"
        >
          <span className="block truncate text-app-label font-semibold text-foreground">{connection.name}</span>
          <Hint text={connection.detail}>
            <span className="block truncate font-mono text-app-caption text-muted-foreground">{connection.detail}</span>
          </Hint>
        </button>
        {action.isPending && <Loader2 size={15} strokeWidth={iconStroke} className="mt-1 animate-spin text-muted-foreground" />}
        {menu}
      </div>
      <div className="mt-auto flex flex-wrap items-center gap-2 px-4 pb-3 pt-3">
        {connection.sourceMissing && <StatusBadge variant="warning">Not found</StatusBadge>}
        {typeBadge}
        <span className="text-app-caption text-muted-foreground">
          {tables}
          {snapshotLabel(connection) ? ` · ${snapshotLabel(connection)}` : ` · added ${relativeTime(connection.createdAt)}`}
        </span>
        {database && (
          <Button variant="outline" size="sm" className="ml-auto h-7" onClick={() => onExplore(connection)}>
            Explore
          </Button>
        )}
      </div>
      {removeDialog}
    </div>
  )
}

function FieldInput({ spec, config, update }: { spec: FieldSpec; config: DbConfig; update: (patch: Partial<DbConfig>) => void }) {
  const core = ['host', 'port', 'database', 'username', 'password'].includes(spec.key)
  const value = core ? String((config as Record<string, unknown>)[spec.key] ?? '') : (config.extra[spec.key] ?? '')
  const set = (next: string) => {
    if (spec.key === 'port') update({ port: Math.min(65535, Number(next.replace(/\D/g, '')) || 0) })
    else if (core) update({ [spec.key]: next } as Partial<DbConfig>)
    else update({ extra: { ...config.extra, [spec.key]: next } })
  }
  const chooseFile = async () => {
    const path = await open({ multiple: false, directory: false })
    if (typeof path === 'string') set(path)
  }
  const id = `field-${spec.key}`
  return (
    <FormField label={spec.label} htmlFor={id} className={spec.wide ? 'col-span-2' : ''}>
      {spec.options ? (
        <Select value={value || spec.options[0].value} onValueChange={set}>
          <SelectTrigger id={id}>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {spec.options.map((option) => (
              <SelectItem key={option.value} value={option.value}>
                {option.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      ) : spec.multiline ? (
        <Textarea
          id={id}
          className="h-24 font-mono text-app-caption"
          value={value}
          autoComplete="off"
          spellCheck={false}
          placeholder={spec.placeholder}
          onChange={(e) => set(e.target.value)}
        />
      ) : (
        <div className="flex gap-2">
          <Input
            id={id}
            type={spec.secret ? 'password' : 'text'}
            value={spec.key === 'port' ? String(config.port) : value}
            autoComplete="off"
            placeholder={spec.placeholder}
            onChange={(e) => set(e.target.value)}
          />
          {spec.file && (
            <Button type="button" variant="outline" onClick={() => void chooseFile()}>
              Browse
            </Button>
          )}
        </div>
      )}
    </FormField>
  )
}

function DatabaseForm({ kind, onSaved }: { kind: DatabaseKind; onSaved: () => void }) {
  const [config, setConfig] = useState<DbConfig>(() => emptyConfig(kind))
  const [result, setResult] = useState<{ ok: boolean; text: string } | null>(null)
  const spec = KINDS[kind]
  const update = (patch: Partial<DbConfig>) => {
    setResult(null)
    setConfig((current) => ({ ...current, ...patch }))
  }
  const test = useMutation({
    mutationFn: api.testDbConnection,
    onSuccess: (message) => setResult({ ok: true, text: message }),
    onError: (failure) => setResult({ ok: false, text: errorMessage(failure) }),
  })
  const save = useMutation({
    mutationFn: api.saveDbConnection,
    onSuccess: (connection) => {
      toast.success(`${connection.name} connected`)
      invalidateConnectionData()
      onSaved()
    },
    onError: (failure) => setResult({ ok: false, text: errorMessage(failure) }),
  })
  const busy = test.isPending || save.isPending

  return (
    <div className="space-y-4">
      <div className="flex items-center gap-2">
        <h3 className="font-serif text-app-title text-foreground">{spec.label}</h3>
        <StatusBadge variant={spec.mode === 'live' ? 'success' : 'warning'}>{spec.mode === 'live' ? 'Live' : 'Snapshot'}</StatusBadge>
      </div>
      <p className="text-app-caption text-muted-foreground">
        {spec.note}
        {spec.mode === 'snapshot' && (
          <Hint text="Rows are copied into this workspace up to the cap (default 1,000,000) and stay as of the import time until you refresh.">
            <span className="cursor-help underline decoration-dotted"> What is a snapshot?</span>
          </Hint>
        )}
      </p>
      <div className="grid grid-cols-2 gap-3">
        <FormField label="Name" htmlFor="conn-name" className="col-span-2">
          <Input id="conn-name" value={config.name} placeholder="e.g. Sales replica" onChange={(e) => update({ name: e.target.value })} />
        </FormField>
        {spec.fields.map((field) => (
          <FieldInput key={`${kind}-${field.key}`} spec={field} config={config} update={update} />
        ))}
      </div>
      {result && (
        <Alert variant={result.ok ? 'default' : 'destructive'}>
          {result.ok ? <Check /> : null}
          <AlertDescription>{result.text}</AlertDescription>
        </Alert>
      )}
      <div className="flex justify-end gap-2 border-t border-border pt-4">
        <Button variant="outline" loading={test.isPending} disabled={busy} onClick={() => test.mutate(config)}>
          Test connection
        </Button>
        <Button loading={save.isPending} disabled={busy} onClick={() => save.mutate(config)}>
          Save connection
        </Button>
      </div>
    </div>
  )
}

type AddChoice = 'files' | 'folder' | DatabaseKind

function AddDataDialog({ open: isOpen, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const add = useAddSources()
  const [choice, setChoice] = useState<AddChoice>('files')
  const [recursive, setRecursive] = useState(true)
  const [search, setSearch] = useState('')

  const close = (next: boolean) => {
    onOpenChange(next)
    if (!next) {
      setChoice('files')
      setSearch('')
    }
  }
  const chooseSources = async (directory: boolean) => {
    const paths = await pickSources(directory)
    if (paths.length === 0) return
    close(false)
    add.mutate({ paths, recursive: directory && recursive })
  }

  const term = search.trim().toLowerCase()
  const kinds = (Object.keys(KINDS) as DatabaseKind[]).filter((kind) => !term || KINDS[kind].label.toLowerCase().includes(term))
  const row = (value: AddChoice, icon: typeof Database, label: string, trailing?: React.ReactNode) => {
    const Icon = icon
    return (
      <button
        key={value}
        type="button"
        onClick={() => setChoice(value)}
        aria-current={choice === value ? 'true' : undefined}
        className={cn(
          'flex h-9 w-full items-center gap-2.5 rounded-lg px-2.5 text-left text-app-label transition-colors duration-fast focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40',
          choice === value ? 'bg-accent text-foreground' : 'text-foreground/80 hover:bg-accent/60 hover:text-foreground',
        )}
      >
        <Icon size={15} strokeWidth={iconStroke} className="shrink-0 text-muted-foreground" />
        <span className="min-w-0 flex-1 truncate">{label}</span>
        {trailing}
      </button>
    )
  }
  const fileish = (choice === 'files' || choice === 'folder') as boolean

  return (
    <Sheet open={isOpen} onOpenChange={close}>
      <SheetContent width="xl" className="flex-row overflow-hidden">
        <aside className="flex w-64 shrink-0 flex-col border-r border-border bg-sidebar">
          <div className="px-5 pb-3 pt-5">
            <DialogTitle>Add data</DialogTitle>
            <DialogDescription className="mt-1">Read where it lives. Nothing is uploaded.</DialogDescription>
          </div>
          <div className="px-3 pb-2">
            <SearchInput value={search} onChange={setSearch} placeholder="Search sources" />
          </div>
          <div className="min-h-0 flex-1 space-y-3 overflow-y-auto px-3 pb-4">
            {!term && (
              <div className="space-y-0.5">
                <p className="px-2.5 pb-1 text-app-caption font-medium text-muted-foreground">From this computer</p>
                {row('files', FileSpreadsheet, 'Files')}
                {row('folder', FolderOpen, 'Folder')}
              </div>
            )}
            {GROUPS.map((group) => {
              const items = kinds.filter((kind) => KINDS[kind].group === group)
              if (items.length === 0) return null
              return (
                <div key={group} className="space-y-0.5">
                  <p className="px-2.5 pb-1 text-app-caption font-medium text-muted-foreground">{group}</p>
                  {items.map((kind) =>
                    row(kind, Database, KINDS[kind].label, <span className={cn('size-1.5 shrink-0 rounded-full', KINDS[kind].mode === 'live' ? 'bg-success' : 'bg-warning')} aria-hidden />),
                  )}
                </div>
              )
            })}
            {term && kinds.length === 0 && <p className="px-2.5 py-1 text-app-caption text-muted-foreground">No sources match.</p>}
          </div>
          <div className="flex gap-3 border-t border-border px-5 py-3 text-app-caption text-muted-foreground">
            <span className="flex items-center gap-1.5">
              <span className="size-1.5 rounded-full bg-success" aria-hidden /> Live
            </span>
            <span className="flex items-center gap-1.5">
              <span className="size-1.5 rounded-full bg-warning" aria-hidden /> Snapshot
            </span>
          </div>
        </aside>
        <div className="min-w-0 flex-1 overflow-y-auto p-6 pr-12">
          {fileish ? (
            <div className="space-y-5">
              <div className="flex items-start gap-3">
                <IconTile icon={choice === 'files' ? FileSpreadsheet : FolderOpen} />
                <div>
                  <h3 className="font-serif text-app-title text-foreground">{choice === 'files' ? 'Files' : 'Folder'}</h3>
                  <p className="text-app-label text-muted-foreground">
                    {choice === 'files'
                      ? 'CSV, TSV, Excel, Parquet or JSON. Each file, or each sheet in a workbook, becomes a table.'
                      : 'Every supported file inside becomes a table. New and changed files update automatically.'}
                  </p>
                </div>
              </div>
              {choice === 'folder' && (
                <label className="flex items-center justify-between gap-3 rounded-lg border border-border px-4 py-3">
                  <span>
                    <span className="block text-app-label text-foreground">Include subfolders</span>
                    <span className="block text-app-caption text-muted-foreground">Also watch every folder inside it.</span>
                  </span>
                  <Switch checked={recursive} onCheckedChange={setRecursive} aria-label="Include subfolders" />
                </label>
              )}
              <div className="rounded-lg border border-dashed border-border px-4 py-6 text-center text-app-caption text-muted-foreground">
                You can also drop files or folders anywhere on the window.
              </div>
              <div className="flex justify-end border-t border-border pt-4">
                <Button onClick={() => void chooseSources(choice === 'folder')}>
                  {choice === 'files' ? <FileSpreadsheet strokeWidth={iconStroke} /> : <FolderOpen strokeWidth={iconStroke} />}
                  {choice === 'files' ? 'Choose files…' : 'Choose folder…'}
                </Button>
              </div>
            </div>
          ) : (
            <DatabaseForm key={choice} kind={choice as DatabaseKind} onSaved={() => close(false)} />
          )}
        </div>
      </SheetContent>
    </Sheet>
  )
}

function ExploreDialog({
  connectionId,
  mode,
  name,
  tables,
  onClose,
}: {
  connectionId: string
  mode: AccessMode
  name: string
  tables: ExploredTable[] | null
  onClose: () => void
}) {
  const [selected, setSelected] = useState(0)
  const [search, setSearch] = useState('')
  const [checked, setChecked] = useState<Set<string>>(new Set())
  const { data: known = [] } = useQuery({ queryKey: queryKeys.tables, queryFn: api.tables })
  const added = new Set(known.filter((item) => item.connection_id === connectionId).map((item) => item.source_ref))
  const term = search.trim().toLowerCase()
  const visible = (tables ?? []).map((item, index) => ({ item, index })).filter(({ item }) => !term || item.name.toLowerCase().includes(term))
  const table = tables?.[selected]
  const columnPg = usePagination(table?.columns ?? [], { initialPageSize: 25, resetKey: selected })
  const toAdd = checked.size > 0 ? [...checked] : table && !added.has(table.name) ? [table.name] : []
  const verb = mode === 'live' ? 'Add' : 'Import'
  const register = useMutation({
    mutationFn: () => api.registerDbTables(connectionId, toAdd),
    onSuccess: (report) => {
      for (const message of report.errors) toast.error(message)
      if (report.tables) toast.success(`${mode === 'live' ? 'Registered' : 'Imported'} ${report.tables} ${mode === 'live' ? 'live ' : ''}table${report.tables === 1 ? '' : 's'}`)
      setChecked(new Set())
      invalidateConnectionData()
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const toggle = (tableName: string) =>
    setChecked((current) => {
      const next = new Set(current)
      if (next.has(tableName)) next.delete(tableName)
      else next.add(tableName)
      return next
    })

  return (
    <Sheet open={tables !== null} onOpenChange={(isOpen) => !isOpen && onClose()}>
      <SheetContent width="xl" className="flex-row overflow-hidden">
        <aside className="flex w-64 shrink-0 flex-col border-r border-border bg-sidebar">
          <p className="px-5 pb-2 pt-5 font-serif text-app-title text-foreground">{name}</p>
          <div className="px-3 pb-2">
            <SearchInput value={search} onChange={setSearch} placeholder="Search tables" />
          </div>
          <div className="min-h-0 flex-1 space-y-0.5 overflow-y-auto px-3 pb-3">
            {visible.length === 0 && <p className="px-2 py-1 text-app-caption text-muted-foreground">No tables match.</p>}
            {visible.map(({ item, index }) => (
              <div
                key={item.name}
                className={`flex h-9 items-center gap-2 rounded-lg px-2 transition-colors duration-fast ${index === selected ? 'bg-accent' : 'hover:bg-accent/60'}`}
              >
                <Checkbox
                  checked={checked.has(item.name) || added.has(item.name)}
                  disabled={added.has(item.name)}
                  onCheckedChange={() => toggle(item.name)}
                  aria-label={`Select ${item.name}`}
                />
                <button
                  type="button"
                  onClick={() => setSelected(index)}
                  aria-current={index === selected ? 'true' : undefined}
                  className="min-w-0 flex-1 truncate rounded text-left font-mono text-app-caption text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40"
                >
                  {item.name}
                </button>
                {added.has(item.name) && <Badge variant="secondary">Added</Badge>}
              </div>
            ))}
          </div>
        </aside>
        <div className="flex min-w-0 flex-1 flex-col">
          <div className="min-h-0 flex-1 space-y-4 overflow-y-auto p-6 pr-12">
            <div className="space-y-1">
              <DialogTitle className="font-mono">{table?.name ?? 'No tables'}</DialogTitle>
              <DialogDescription>
                {table ? `${table.row_count >= 0 ? `${table.row_count.toLocaleString()} rows` : 'Rows not counted'} · read-only preview` : 'This database has no tables you can read.'}
              </DialogDescription>
            </div>
            {table && (
              <>
                <div className="flex flex-wrap gap-1.5">
                  {columnPg.pageItems.map((column) => (
                    <Badge key={column.name} variant="outline" className="font-mono">
                      {column.name} <span className="ml-1 text-muted-foreground">{column.data_type}</span>
                    </Badge>
                  ))}
                </div>
                <PaginationBar page={columnPg.page} pageSize={columnPg.pageSize} total={columnPg.total} onPageChange={columnPg.setPage} onPageSizeChange={columnPg.setPageSize} className="" />
                <SampleTable columns={table.columns.map((column) => column.name)} rows={table.samples} />
              </>
            )}
          </div>
          <div className="flex items-center justify-between gap-3 border-t border-border px-6 py-3">
            <p className="text-app-caption text-muted-foreground">
              <Hint
                text={
                  mode === 'live'
                    ? 'Registers a read-only view. Queries still go through the SQL guard. Nothing is copied.'
                    : 'Copies the rows once (up to the row cap) as a read-only snapshot, stamped with the import time. Use Refresh on the connection to re-import.'
                }
              >
                <span className="cursor-help">{mode === 'live' ? 'Read-only live view; nothing is copied.' : 'Read-only snapshot; use Refresh to re-import.'}</span>
              </Hint>
            </p>
            <Button size="sm" loading={register.isPending} disabled={toAdd.length === 0} onClick={() => register.mutate()}>
              {toAdd.length > 1 ? `${verb} ${toAdd.length} tables` : mode === 'live' ? 'Add as live table' : 'Import snapshot'}
            </Button>
          </div>
        </div>
      </SheetContent>
    </Sheet>
  )
}

export function ConnectionsPage() {
  const { data: connections = [], error, isPending } = useQuery({ queryKey: queryKeys.connections, queryFn: api.connections })
  const pending = usePendingSources()
  const [addOpen, setAddOpen] = useState(false)
  const [search, setSearch] = useState('')
  const [view, setView] = useViewMode('connections')
  const [explored, setExplored] = useState<{ id: string; name: string; mode: AccessMode; tables: ExploredTable[] } | null>(null)
  const explore = useMutation({
    mutationFn: (connection: Connection) => api.exploreDbConnection(connection.id),
    onSuccess: (tables, connection) => setExplored({ id: connection.id, name: connection.name, mode: connection.mode, tables }),
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const term = search.trim().toLowerCase()
  const shown = (term ? connections.filter((item) => `${item.name} ${item.detail}`.toLowerCase().includes(term)) : connections)
    .slice()
    .sort((a, b) => Number(isDatabase(a)) - Number(isDatabase(b)))
  const pg = usePagination(shown, { initialPageSize: 12, resetKey: term })

  return (
    <PageContainer width="full">
      <PageHeader
        title="Connections"
        description="Data sources in this workspace. Everything is read where it lives."
        actions={
          <Button onClick={() => setAddOpen(true)}>
            <Plus strokeWidth={iconStroke} /> Add data
          </Button>
        }
      />
      {error && <p className="text-app-label text-destructive">{errorMessage(error)}</p>}
      {pending.length > 0 && <PendingSources paths={pending} />}
      {isPending ? (
        <PageLoader label="Loading connections" />
      ) : connections.length === 0 && pending.length === 0 ? (
        <EmptyState
          icon={Database}
          title="Connect your first data"
          description="Add a file, a folder or a database. Everything is read where it lives, and nothing leaves this computer."
          action={
            <Button onClick={() => setAddOpen(true)}>
              <Plus strokeWidth={iconStroke} /> Add data
            </Button>
          }
        />
      ) : (
        connections.length > 0 && (
          <section className="space-y-4">
            <StatRow
              stats={[
                { label: 'Sources', value: connections.length, icon: Database },
                { label: 'Tables', value: connections.reduce((sum, item) => sum + item.tableCount, 0), icon: Table2 },
                { label: 'Files and folders', value: connections.filter((item) => !isDatabase(item)).length, icon: FileSpreadsheet },
                { label: 'Databases', value: connections.filter(isDatabase).length, hint: `${connections.filter((item) => isDatabase(item) && item.mode === 'live').length} live`, icon: Database },
              ]}
            />
            <SectionHeader
              title="Sources"
              description="Select a source to see its tables."
              actions={
                <div className="flex items-center gap-2">
                  <SearchInput value={search} onChange={setSearch} placeholder="Search sources" className="w-64" />
                  <ViewToggle value={view} onChange={setView} />
                </div>
              }
            />
            {shown.length === 0 ? (
              <p className="text-app-label text-muted-foreground">No sources match “{search}”.</p>
            ) : view === 'cards' ? (
              <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-3">
                {pg.pageItems.map((connection) => (
                  <SourceRow key={connection.id} connection={connection} onExplore={(item) => explore.mutate(item)} />
                ))}
              </div>
            ) : (
              <div className="overflow-hidden rounded-xl border border-border bg-surface">
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Source</TableHead>
                      <TableHead>Type</TableHead>
                      <TableHead>Tables</TableHead>
                      <TableHead>Updated</TableHead>
                      <TableHead className="w-12">
                        <span className="sr-only">Actions</span>
                      </TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {pg.pageItems.map((connection) => (
                      <SourceRow key={connection.id} layout="row" connection={connection} onExplore={(item) => explore.mutate(item)} />
                    ))}
                  </TableBody>
                </Table>
              </div>
            )}
            <PaginationBar alwaysShow page={pg.page} pageSize={pg.pageSize} total={pg.total} onPageChange={pg.setPage} onPageSizeChange={pg.setPageSize} className="pt-1" />
          </section>
        )
      )}
      <AddDataDialog open={addOpen} onOpenChange={setAddOpen} />
      <ExploreDialog
        key={explored?.id ?? explored?.name}
        connectionId={explored?.id ?? ''}
        mode={explored?.mode ?? 'live'}
        name={explored?.name ?? ''}
        tables={explored?.tables ?? null}
        onClose={() => setExplored(null)}
      />
    </PageContainer>
  )
}
