import { useEffect, useState } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { Boxes, Cable, Cpu, Eye, EyeOff, Palette, Plus, Shield, Trash2 } from 'lucide-react'
import { open, save } from '@tauri-apps/plugin-dialog'
import { toast } from 'sonner'
import { api, errorMessage, type TaskRouting, type Workspace } from '../../lib/api'
import { PaginationBar } from '../ui/pagination'
import { usePagination } from '../../lib/usePagination'
import { iconStroke } from '../../lib/icon'
import { lockNow } from '../../lib/lock'
import { queryClient, queryKeys } from '../../lib/queryClient'
import { useAppearance, type FontSize, type ThemeMode } from '../../lib/theme'
import { cn } from '../../lib/utils'
import { useWorkspaceAction, useWorkspaces } from '../../lib/workspaces'
import { useNavigation, type SettingsSectionId } from '../../stores/navigation'
import { ModelManager } from '../models/ModelManager'
import { NewWorkspaceDialog } from '../workspaces/NewWorkspaceDialog'
import { SegmentedControl, SettingsAction, SettingsCard, SettingsRow, SettingsSection, StatusBadge } from '../layout/ui'
import { ConfirmAction } from '../ui/alert-dialog'
import { Button } from '../ui/button'
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '../ui/dialog'
import { Input } from '../ui/input'
import { Switch } from '../ui/switch'
import { Checkbox } from '../ui/checkbox'
import { Label } from '../ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../ui/select'

const SECTIONS: { group: string; items: { id: SettingsSectionId; label: string; description: string; icon: typeof Cpu }[] }[] = [
  { group: 'App', items: [{ id: 'general', label: 'General', description: 'Appearance and this computer.', icon: Palette }] },
  {
    group: 'Analysis',
    items: [
      { id: 'models', label: 'AI models', description: 'Local models that answer questions. Nothing leaves this computer.', icon: Cpu },
      { id: 'workspaces', label: 'Workspaces', description: 'Separate sets of data, each stored on its own.', icon: Boxes },
    ],
  },
  {
    group: 'Trust',
    items: [
      { id: 'security', label: 'Security', description: 'Lock, offline mode, local users, encryption and backup.', icon: Shield },
      { id: 'integrations', label: 'Integrations', description: 'MCP servers your analysts can call. Outbound is labelled.', icon: Cable },
    ],
  },
]

function BrandSection() {
  const { data: brand } = useQuery({ queryKey: queryKeys.theme, queryFn: api.theme })
  const [name, setName] = useState('')
  const [color, setColor] = useState('#C96240')
  useEffect(() => {
    if (!brand) return
    setName(brand.name)
    setColor(brand.brand)
  }, [brand])
  const save = useMutation({
    mutationFn: () => api.setTheme(color, name),
    onSuccess: (theme) => {
      setName(theme.name)
      setColor(theme.brand)
      void queryClient.invalidateQueries({ queryKey: queryKeys.theme })
      toast.success('Brand saved')
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  return (
    <SettingsSection collapsible title={'Brand'} description={'Used on datasites, reports and decks.'}>
      <SettingsCard>
        <SettingsRow label={'Name'}>
          <Input value={name} onChange={(event) => setName(event.target.value)} aria-label={'Brand name'} className="max-w-[12rem]" />
        </SettingsRow>
        <SettingsRow label={'Colour'}>
          <Input value={color} onChange={(event) => setColor(event.target.value)} aria-label={'Brand colour'} className="max-w-[8rem]" />
        </SettingsRow>
        <SettingsAction>
          <Button size="sm" disabled={!name.trim() || save.isPending} onClick={() => save.mutate()}>
            {'Save brand'}
          </Button>
        </SettingsAction>
      </SettingsCard>
    </SettingsSection>
  )
}

function SmtpSection() {
  const { data } = useQuery({ queryKey: queryKeys.smtp, queryFn: api.smtpGet })
  const [host, setHost] = useState('')
  const [port, setPort] = useState(587)
  const [username, setUsername] = useState('')
  const [from, setFrom] = useState('')
  const [password, setPassword] = useState('')
  const [enabled, setEnabled] = useState(false)
  useEffect(() => {
    if (!data) return
    setHost(data.host)
    setPort(data.port)
    setUsername(data.username)
    setFrom(data.from)
    setEnabled(data.enabled)
  }, [data])
  const save = useMutation({
    mutationFn: () => api.smtpSave({ enabled, host, port, username, from, password: password || undefined }),
    onSuccess: () => {
      setPassword('')
      void queryClient.invalidateQueries({ queryKey: queryKeys.smtp })
      toast.success('SMTP saved')
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  return (
    <SettingsSection collapsible title={'Email'} description={'Off by default. Sending mail leaves this device. The password is stored in your system keychain.'}>
      <SettingsCard>
        <SettingsRow label={'Send mail'}>
          <Switch checked={enabled} onCheckedChange={setEnabled} aria-label={'Enable SMTP'} />
        </SettingsRow>
        <SettingsRow label={'Host'}>
          <Input value={host} onChange={(event) => setHost(event.target.value)} className="max-w-[16rem]" aria-label={'SMTP host'} />
        </SettingsRow>
        <SettingsRow label={'Port'}>
          <Input value={String(port)} onChange={(event) => setPort(Number(event.target.value) || 587)} className="max-w-[6rem]" aria-label={'SMTP port'} />
        </SettingsRow>
        <SettingsRow label={'Username'}>
          <Input value={username} onChange={(event) => setUsername(event.target.value)} className="max-w-[16rem]" aria-label={'SMTP username'} />
        </SettingsRow>
        <SettingsRow label={'From'}>
          <Input value={from} onChange={(event) => setFrom(event.target.value)} className="max-w-[16rem]" aria-label={'SMTP from'} />
        </SettingsRow>
        <SettingsRow label={'Password'} description={data?.hasPassword ? 'A password is already in the keychain.' : 'Required to send.'}>
          <Input type="password" value={password} onChange={(event) => setPassword(event.target.value)} className="max-w-[16rem]" aria-label={'SMTP password'} />
        </SettingsRow>
        <SettingsAction>
          <Button size="sm" disabled={save.isPending} onClick={() => save.mutate()}>
            {'Save SMTP'}
          </Button>
        </SettingsAction>
      </SettingsCard>
    </SettingsSection>
  )
}

function GeneralSection() {
  const { theme, setTheme, fontSize, setFontSize } = useAppearance()
  const { data: status } = useQuery({ queryKey: queryKeys.status, queryFn: api.appStatus })
  const hardware = status?.hardware

  return (
    <div className="space-y-8">
      <SmtpSection />
      <SettingsSection title={'Appearance'}>
        <SettingsCard>
          <SettingsRow label={'Theme'}>
            <SegmentedControl<ThemeMode>
              value={theme}
              onChange={setTheme}
              options={[
                { value: 'light', label: 'Light' },
                { value: 'dark', label: 'Dark' },
                { value: 'system', label: 'System' },
              ]}
            />
          </SettingsRow>
          <SettingsRow label={'Text size'}>
            <SegmentedControl<FontSize>
              value={fontSize}
              onChange={setFontSize}
              options={[
                { value: 'sm', label: 'Small' },
                { value: 'md', label: 'Default' },
                { value: 'lg', label: 'Large' },
              ]}
            />
          </SettingsRow>
        </SettingsCard>
      </SettingsSection>
      <BrandSection />
      {hardware && status && (
        <SettingsSection title={'This computer'}>
          <SettingsCard>
            <SettingsRow label={'Memory'}>
              <span className="text-app-label text-muted-foreground">{hardware.ramGb} GB</span>
            </SettingsRow>
            <SettingsRow label={'Processor'}>
              <span className="text-app-label text-muted-foreground">
                {hardware.cpu} · {hardware.cores} cores
              </span>
            </SettingsRow>
            <SettingsRow label={'Acceleration'}>
              <span className="text-app-label text-muted-foreground">{hardware.accelerator}</span>
            </SettingsRow>
            <SettingsRow label={'LocalBoard version'}>
              <span className="text-app-label text-muted-foreground">{status.version}</span>
            </SettingsRow>
          </SettingsCard>
        </SettingsSection>
      )}
    </div>
  )
}

const TASKS: { key: keyof TaskRouting; label: string; description: string }[] = [
  { key: 'sql', label: 'Writing SQL', description: 'Turns questions into queries.' },
  { key: 'reasoning', label: 'Reasoning', description: 'Plans multi-step analysis.' },
  { key: 'narrative', label: 'Explanations', description: 'Summarises results in plain words.' },
]

function TaskRoutingSection() {
  const { data: catalog } = useQuery({ queryKey: queryKeys.models, queryFn: api.modelCatalog })
  const save = useMutation({
    mutationFn: api.setTaskRouting,
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.models }),
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const stop = useMutation({
    mutationFn: api.stopModel,
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: queryKeys.models }),
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const installed = catalog?.models.filter((model) => model.installed) ?? []
  if (!catalog || installed.length === 0) return null
  const running = catalog.models.find((model) => model.id === catalog.running)

  return (
    <SettingsSection collapsible title={'Model per task'} description={'Use a faster model for SQL and a stronger one for explanations, if you have both.'}>
      <SettingsCard>
        {TASKS.map((task) => (
          <SettingsRow key={task.key} label={task.label} description={task.description}>
            <Select
              value={catalog.routing[task.key] ?? 'auto'}
              onValueChange={(value) => save.mutate({ ...catalog.routing, [task.key]: value === 'auto' ? null : value })}
            >
              <SelectTrigger className="w-56" aria-label={task.label}>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="auto">Automatic</SelectItem>
                {installed.map((model) => (
                  <SelectItem key={model.id} value={model.id}>
                    {model.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </SettingsRow>
        ))}
        <SettingsRow
          label={'Loaded model'}
          description={running ? 'Unload it to free memory; it reloads on the next question.' : 'No model is in memory.'}
        >
          {running ? (
            <Button variant="outline" size="sm" disabled={stop.isPending} onClick={() => stop.mutate()}>
              Unload {running.name}
            </Button>
          ) : (
            <StatusBadge variant="neutral">Idle</StatusBadge>
          )}
        </SettingsRow>
      </SettingsCard>
    </SettingsSection>
  )
}

function WorkspaceRow({ workspace, active }: { workspace: Workspace; active: boolean }) {
  const action = useWorkspaceAction()
  const [name, setName] = useState(workspace.name)
  const rename = () => {
    const next = name.trim()
    if (next === workspace.name) return
    if (!next) return setName(workspace.name)
    action.mutate(() => api.renameWorkspace(workspace.id, next), { onError: () => setName(workspace.name) })
  }

  return (
    <div className="flex items-center gap-3 px-4 py-3">
      <Input
        aria-label={'Workspace name'}
        value={name}
        maxLength={80}
        onChange={(event) => setName(event.target.value)}
        onBlur={rename}
        onKeyDown={(event) => event.key === 'Enter' && event.currentTarget.blur()}
        className="h-8 flex-1 border-transparent bg-transparent shadow-none hover:border-border focus:border-border"
      />
      {active ? (
        <StatusBadge variant="success">Open</StatusBadge>
      ) : (
        <>
          <Button variant="ghost" size="sm" onClick={() => action.mutate(() => api.switchWorkspace(workspace.id))}>
            {'Open'}
          </Button>
          <ConfirmAction
            title={`Delete “${workspace.name}”?`}
            description={'Its connections, table descriptions, history and saved passwords are removed from this computer. Your source files are not touched.'}
            confirmLabel={'Delete workspace'}
            onConfirm={() => action.mutate(() => api.deleteWorkspace(workspace.id))}
            trigger={
              <Button type="button" variant="ghost" size="icon" aria-label={`Delete ${workspace.name}`}>
                <Trash2 strokeWidth={iconStroke} />
              </Button>
            }
          />
        </>
      )}
    </div>
  )
}

function WorkspacesSection() {
  const { data } = useWorkspaces()
  const [creating, setCreating] = useState(false)
  const workspacePg = usePagination(data?.workspaces ?? [], { initialPageSize: 10 })
  if (!data) return null

  return (
    <SettingsSection>
      <SettingsCard>
        {workspacePg.pageItems.map((workspace) => (
          <WorkspaceRow key={workspace.id} workspace={workspace} active={workspace.id === data.activeId} />
        ))}
      </SettingsCard>
      <PaginationBar page={workspacePg.page} pageSize={workspacePg.pageSize} total={workspacePg.total} onPageChange={workspacePg.setPage} onPageSizeChange={workspacePg.setPageSize} className="pt-1" />
      <Button variant="outline" size="sm" onClick={() => setCreating(true)}>
        <Plus strokeWidth={iconStroke} /> New workspace
      </Button>
      <NewWorkspaceDialog open={creating} onOpenChange={setCreating} />
    </SettingsSection>
  )
}

function SecuritySection() {
  const { data: gov } = useQuery({ queryKey: queryKeys.governance, queryFn: api.governance })
  const [lockPass, setLockPass] = useState('')
  const [lockCurrent, setLockCurrent] = useState('')
  const [rememberKey, setRememberKey] = useState(false)
  const [importPath, setImportPath] = useState<string | null>(null)
  const [importPass, setImportPass] = useState('')
  const [importRemember, setImportRemember] = useState(false)
  const [encryptPass, setEncryptPass] = useState('')
  const invalidate = () => {
    void queryClient.invalidateQueries({ queryKey: queryKeys.governance })
    void queryClient.invalidateQueries({ queryKey: queryKeys.status })
  }
  const offline = useMutation({
    mutationFn: api.setOffline,
    onSuccess: () => invalidate(),
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const lock = useMutation({
    mutationFn: (enabled: boolean) => api.setAppLock(enabled, lockPass, lockCurrent || undefined),
    onSuccess: () => {
      setLockPass('')
      setLockCurrent('')
      invalidate()
      toast.success('App lock updated')
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const encrypt = useMutation({
    mutationFn: () => api.encryptWorkspace(encryptPass, rememberKey),
    onSuccess: () => {
      setEncryptPass('')
      invalidate()
      toast.success('Workspace encrypted')
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const backupOut = useMutation({
    mutationFn: async () => {
      const path = await save({ defaultPath: 'localboard-backup.zip', filters: [{ name: 'Zip', extensions: ['zip'] }] })
      if (!path) return
      await api.backupExport(path)
    },
    onSuccess: () => toast.success('Backup written'),
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const backupIn = useMutation({
    mutationFn: async () => {
      const path = await open({ multiple: false, filters: [{ name: 'Zip', extensions: ['zip'] }] })
      if (typeof path !== 'string') return
      setImportPath(path)
      await api.backupImport(path, 'Restored workspace')
    },
    onSuccess: () => {
      setImportPath(null)
      void queryClient.invalidateQueries({ queryKey: queryKeys.workspaces })
      toast.success('Backup imported')
    },
    onError: (failure) => {
      // An encrypted backup asks for its passphrase; the picked path is kept for the retry.
      if (!/encrypted/i.test(errorMessage(failure))) setImportPath(null)
      toast.error(errorMessage(failure))
    },
  })
  const backupInEncrypted = useMutation({
    mutationFn: async () => {
      if (!importPath) return
      await api.backupImport(importPath, 'Restored workspace', importPass, importRemember)
    },
    onSuccess: () => {
      setImportPath(null)
      setImportPass('')
      void queryClient.invalidateQueries({ queryKey: queryKeys.workspaces })
      toast.success('Backup imported')
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  return (
    <div className="space-y-8">
      <SettingsSection title={'Offline'} description={'Blocks Hugging Face downloads, SMTP and remote MCP servers.'}>
        <SettingsCard>
          <SettingsRow label={'Offline mode'}>
            <Switch checked={Boolean(gov?.offline)} onCheckedChange={(enabled) => offline.mutate(enabled)} aria-label={'Offline mode'} />
          </SettingsRow>
        </SettingsCard>
      </SettingsSection>
      <SettingsSection title={'App lock'} description={'A passphrase on this computer. Unlock each launch.'}>
        <SettingsCard>
          {gov?.lockEnabled && (
            <SettingsRow label={'Current passphrase'} description={'Needed to change or remove the lock.'}>
              <Input type="password" value={lockCurrent} onChange={(event) => setLockCurrent(event.target.value)} className="max-w-[14rem]" aria-label={'Current app lock passphrase'} />
            </SettingsRow>
          )}
          <SettingsRow label={gov?.lockEnabled ? 'New passphrase' : 'Passphrase'}>
            <Input type="password" value={lockPass} onChange={(event) => setLockPass(event.target.value)} className="max-w-[14rem]" aria-label={'App lock passphrase'} />
          </SettingsRow>
          <SettingsRow label={gov?.lockEnabled ? 'Change or turn off' : 'Turn on'} description={'Cmd/Ctrl-L locks the app right away.'}>
            <div className="flex gap-2">
              <Button size="sm" disabled={!lockPass || (gov?.lockEnabled && !lockCurrent) || lock.isPending} onClick={() => lock.mutate(true)}>
                {gov?.lockEnabled ? 'Change passphrase' : 'Set lock'}
              </Button>
              {gov?.lockEnabled && (
                <>
                  <Button size="sm" variant="outline" disabled={!lockCurrent || lock.isPending} onClick={() => lock.mutate(false)}>
                    {'Remove lock'}
                  </Button>
                  <Button size="sm" variant="outline" onClick={() => void lockNow()}>
                    {'Lock now'}
                  </Button>
                </>
              )}
            </div>
          </SettingsRow>
        </SettingsCard>
      </SettingsSection>
      <SettingsSection collapsible title={'Encryption'} description={`SQLCipher, opt-in. Existing workspaces stay unencrypted until you turn this on. ${gov?.encryptionScope ?? ''}`}>
        <SettingsCard>
          <SettingsRow label={'Passphrase'} description={gov?.encrypted ? 'This workspace is encrypted.' : undefined}>
            <Input type="password" value={encryptPass} onChange={(event) => setEncryptPass(event.target.value)} className="max-w-[14rem]" disabled={Boolean(gov?.encrypted)} aria-label={'Encryption passphrase'} />
          </SettingsRow>
          <SettingsRow label={'Remember'} description={'Store the key in the OS keychain so this workspace opens without asking.'}>
            <Switch checked={rememberKey} onCheckedChange={setRememberKey} disabled={Boolean(gov?.encrypted)} aria-label={'Remember workspace key'} />
          </SettingsRow>
          <SettingsAction>
            <Button size="sm" disabled={Boolean(gov?.encrypted) || encryptPass.length < 8 || encrypt.isPending} onClick={() => encrypt.mutate()}>
              {'Encrypt this workspace'}
            </Button>
          </SettingsAction>
        </SettingsCard>
      </SettingsSection>
      <SettingsSection collapsible title={'Backup'} description={'Exports the workspace SQLite and DuckDB files. Secrets stay in the keychain.'}>
        <SettingsCard>
          <SettingsRow label={'Export'}>
            <Button size="sm" variant="outline" disabled={backupOut.isPending} onClick={() => backupOut.mutate()}>
              {'Export zip'}
            </Button>
          </SettingsRow>
          <SettingsRow label={'Import'}>
            <Button size="sm" variant="outline" disabled={backupIn.isPending} onClick={() => backupIn.mutate()}>
              {'Import zip'}
            </Button>
          </SettingsRow>
          {importPath && (
            <SettingsRow label={'Backup passphrase'} description={'This backup is encrypted. Enter its passphrase.'}>
              <div className="flex flex-wrap items-center gap-2">
                <Input type="password" value={importPass} onChange={(event) => setImportPass(event.target.value)} className="w-44" aria-label={'Backup passphrase'} />
                <div className="flex items-center gap-1.5">
                  <Checkbox id="import-remember" checked={importRemember} onCheckedChange={(value) => setImportRemember(value === true)} />
                  <Label htmlFor="import-remember" className="text-app-caption font-normal text-muted-foreground">
                    {'Remember'}
                  </Label>
                </div>
                <Button size="sm" disabled={!importPass || backupInEncrypted.isPending} onClick={() => backupInEncrypted.mutate()}>
                  {'Import'}
                </Button>
                <Button size="sm" variant="outline" onClick={() => setImportPath(null)}>
                  {'Cancel'}
                </Button>
              </div>
            </SettingsRow>
          )}
        </SettingsCard>
      </SettingsSection>
    </div>
  )
}

function LocalMcpSection() {
  const { data: info } = useQuery({ queryKey: ['local-mcp'], queryFn: api.localMcpInfo })
  const [reveal, setReveal] = useState(false)
  const rotate = useMutation({
    mutationFn: api.localMcpRotateToken,
    onSuccess: (next) => {
      queryClient.setQueryData(['local-mcp'], next)
      toast.success('Token rotated. Update your clients.')
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  if (!info) return null
  const copy = (text: string) => navigator.clipboard.writeText(text).then(() => toast.success('Copied'), () => toast.error('Could not copy'))
  const snippet = JSON.stringify({ mcpServers: { localboard: { command: info.proxyCommand.split(' ')[0], args: info.proxyCommand.split(' ').slice(1) } } }, null, 2)
  const httpSnippet = JSON.stringify({ mcpServers: { localboard: { url: info.url, headers: { Authorization: `Bearer ${info.token}` } } } }, null, 2)
  return (
    <SettingsSection collapsible title={'LocalBoard as an MCP server'} description={'Lets Claude Desktop or Cursor query this workspace. Local only; the token is the key.'}>
      <SettingsCard>
        <SettingsRow label={'Endpoint'}>
          <div className="flex items-center gap-2">
            <code className="text-app-caption text-foreground">{info.url}</code>
            <Button size="sm" variant="outline" onClick={() => void copy(info.url)}>
              {'Copy'}
            </Button>
          </div>
        </SettingsRow>
        <SettingsRow label={'Token'}>
          <div className="flex items-center gap-2">
            <code className="max-w-[14rem] truncate text-app-caption text-foreground">{reveal ? info.token : '•'.repeat(24)}</code>
            <Button size="sm" variant="outline" aria-label={reveal ? 'Hide token' : 'Reveal token'} onClick={() => setReveal(!reveal)}>
              {reveal ? <EyeOff size={14} strokeWidth={iconStroke} /> : <Eye size={14} strokeWidth={iconStroke} />}
            </Button>
            <Button size="sm" variant="outline" onClick={() => void copy(info.token)}>
              {'Copy'}
            </Button>
            <ConfirmAction
              title={'Rotate the token?'}
              description={'Every connected client stops working until you give it the new token.'}
              confirmLabel={'Rotate'}
              onConfirm={() => rotate.mutate()}
              trigger={
                <Button size="sm" variant="outline" disabled={rotate.isPending}>
                  {'Rotate'}
                </Button>
              }
            />
          </div>
        </SettingsRow>
        <SettingsRow label={'Claude Desktop / Cursor'} description={'Stdio proxy. Paste into claude_desktop_config.json or .cursor/mcp.json.'}>
          <Button size="sm" variant="outline" onClick={() => void copy(snippet)}>
            {'Copy config'}
          </Button>
        </SettingsRow>
        <SettingsRow label={'HTTP clients'} description={'For clients that connect by URL. Contains the token.'}>
          <Button size="sm" variant="outline" onClick={() => void copy(httpSnippet)}>
            {'Copy config'}
          </Button>
        </SettingsRow>
      </SettingsCard>
      <pre className="overflow-x-auto rounded-lg border border-border bg-card p-3 text-app-caption text-foreground">{snippet}</pre>
    </SettingsSection>
  )
}

function IntegrationsSection() {
  const { data: servers = [] } = useQuery({ queryKey: queryKeys.mcpServers, queryFn: api.mcpServers })
  const serverPg = usePagination(servers, { initialPageSize: 10 })
  const [name, setName] = useState('')
  const [transport, setTransport] = useState('stdio')
  const [command, setCommand] = useState('')
  const [url, setUrl] = useState('')
  const removeServer = useMutation({
    mutationFn: (id: string) => api.mcpDeleteServer(id),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.mcpServers })
      toast.success('MCP server removed')
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  const saveServer = useMutation({
    mutationFn: () => api.mcpSaveServer({ name, transport, command: command || undefined, url: url || undefined, enabled: true }),
    onSuccess: () => {
      setName('')
      setCommand('')
      setUrl('')
      void queryClient.invalidateQueries({ queryKey: queryKeys.mcpServers })
      toast.success('MCP server saved')
    },
    onError: (failure) => toast.error(errorMessage(failure)),
  })
  return (
    <div className="space-y-8">
      <LocalMcpSection />
      <SettingsSection collapsible title={'MCP servers'} description={'stdio or HTTP. Tools are live, read-only unless you allow a write on an analyst.'}>
        <SettingsCard>
          <SettingsRow label={'Name'}>
            <Input value={name} onChange={(event) => setName(event.target.value)} className="max-w-[12rem]" aria-label={'MCP server name'} />
          </SettingsRow>
          <SettingsRow label={'Transport'}>
            <Input value={transport} onChange={(event) => setTransport(event.target.value)} className="max-w-[8rem]" aria-label={'MCP transport'} />
          </SettingsRow>
          <SettingsRow label={'Command'}>
            <Input value={command} onChange={(event) => setCommand(event.target.value)} className="max-w-[16rem]" aria-label={'MCP command'} />
          </SettingsRow>
          <SettingsRow label={'URL'}>
            <Input value={url} onChange={(event) => setUrl(event.target.value)} className="max-w-[16rem]" aria-label={'MCP URL'} />
          </SettingsRow>
          <SettingsAction>
            <Button size="sm" disabled={!name.trim() || saveServer.isPending} onClick={() => saveServer.mutate()}>
              {'Add server'}
            </Button>
          </SettingsAction>
        </SettingsCard>
        {servers.length > 0 && (
          <>
          <SettingsCard>
            {serverPg.pageItems.map((server) => (
              <SettingsRow key={server.id} label={server.name} description={server.transport}>
                <ConfirmAction
                  trigger={
                    <Button variant="ghost" size="icon" aria-label={`Remove ${server.name}`}>
                      <Trash2 strokeWidth={iconStroke} />
                    </Button>
                  }
                  title={`Remove “${server.name}”?`}
                  description="The server and its tool rules are removed. Analysts can no longer call its tools."
                  confirmLabel="Remove server"
                  onConfirm={() => removeServer.mutate(server.id)}
                />
              </SettingsRow>
            ))}
          </SettingsCard>
          <PaginationBar page={serverPg.page} pageSize={serverPg.pageSize} total={serverPg.total} onPageChange={serverPg.setPage} onPageSizeChange={serverPg.setPageSize} className="pt-1" />
          </>
        )}
      </SettingsSection>
    </div>
  )
}

function SectionContent({ section }: { section: SettingsSectionId }) {
  switch (section) {
    case 'general':
      return <GeneralSection />
    case 'models':
      return (
        <div className="space-y-8">
          <ModelManager />
          <TaskRoutingSection />
        </div>
      )
    case 'workspaces':
      return <WorkspacesSection />
    case 'security':
      return <SecuritySection />
    case 'integrations':
      return <IntegrationsSection />
  }
}

export function SettingsModal() {
  const { settingsOpen, settingsSection, closeSettings, setSettingsSection } = useNavigation()
  const current = SECTIONS.flatMap((group) => group.items).find((item) => item.id === settingsSection)!

  return (
    <Dialog open={settingsOpen} onOpenChange={(open) => !open && closeSettings()}>
      <DialogContent className="flex h-[min(640px,85vh)] max-w-4xl gap-0 overflow-hidden p-0">
        <aside className="flex w-[14rem] shrink-0 flex-col border-r border-border bg-sidebar p-3">
          <p className="px-2 pb-4 pt-2 font-serif text-app-title text-foreground">Settings</p>
          <nav className="space-y-4">
            {SECTIONS.map((group) => (
              <div key={group.group} className="space-y-0.5">
                <p className="px-2 pb-1 text-app-caption text-muted-foreground">{group.group}</p>
                {group.items.map(({ id, label, icon: Icon }) => (
                  <button
                    key={id}
                    type="button"
                    onClick={() => setSettingsSection(id)}
                    className={cn(
                      'flex h-8 w-full items-center gap-2 rounded-lg px-2 text-app-label transition-colors duration-fast',
                      settingsSection === id ? 'bg-accent text-foreground' : 'text-foreground/80 hover:bg-accent hover:text-foreground',
                    )}
                  >
                    <Icon size={15} strokeWidth={iconStroke} />
                    {label}
                  </button>
                ))}
              </div>
            ))}
          </nav>
        </aside>
        <div className="flex min-w-0 flex-1 flex-col">
          <header className="space-y-1 px-8 pb-5 pt-7">
            <DialogTitle >{current.label}</DialogTitle>
            <DialogDescription>{current.description}</DialogDescription>
          </header>
          <div className="min-h-0 flex-1 overflow-y-auto px-8 pb-8">
            <SectionContent section={settingsSection} />
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}

