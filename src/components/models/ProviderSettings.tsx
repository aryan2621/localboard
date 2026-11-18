import { useEffect, useState } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { toast } from 'sonner'
import { api, errorMessage, type LlmKind, type LlmSettings, type ModelParams } from '../../lib/api'
import { queryClient, queryKeys } from '../../lib/queryClient'
import { Button } from '../ui/button'
import { Input } from '../ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../ui/select'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../ui/tabs'
import { FormField } from '../layout/ui'

const KINDS: { kind: LlmKind; label: string; baseUrl: string }[] = [
  { kind: 'anthropic', label: 'Anthropic', baseUrl: 'https://api.anthropic.com' },
  { kind: 'openai', label: 'OpenAI', baseUrl: 'https://api.openai.com/v1' },
  { kind: 'gemini', label: 'Google Gemini', baseUrl: 'https://generativelanguage.googleapis.com/v1beta/openai' },
  { kind: 'ollama', label: 'Ollama (this computer)', baseUrl: 'http://localhost:11434/v1' },
  { kind: 'compat', label: 'OpenAI-compatible server', baseUrl: '' },
]

const NO_PARAMS: ModelParams = { temperature: null, top_p: null, num_ctx: null, timeout_secs: null }
const toNumber = (text: string) => (text.trim() === '' || Number.isNaN(Number(text)) ? null : Number(text))

/** A blank box means "use the default", which is stored as null. */
function ParamField({ id, label, hint, value, onChange }: { id: string; label: string; hint: string; value: number | null; onChange: (value: number | null) => void }) {
  return (
    <FormField label={label} htmlFor={id} hint={hint}>
      <Input id={id} inputMode="decimal" placeholder="Default" value={value ?? ''} onChange={(event) => onChange(toNumber(event.target.value))} />
    </FormField>
  )
}

/** Provider, model and key for the AI analyst. Shared by Settings and Onboarding. */
export function ProviderSettings() {
  const { data: config } = useQuery({ queryKey: queryKeys.models, queryFn: api.llmConfig })
  const [kind, setKind] = useState<LlmKind>('anthropic')
  const [baseUrl, setBaseUrl] = useState(KINDS[0].baseUrl)
  const [model, setModel] = useState('')
  const [embedModel, setEmbedModel] = useState('')
  const [params, setParams] = useState<ModelParams>(NO_PARAMS)
  const [apiKey, setApiKey] = useState('')
  const [installed, setInstalled] = useState<string[]>([])
  const [testResult, setTestResult] = useState<{ ok: boolean; message: string } | null>(null)

  useEffect(() => {
    if (!config) return
    setKind(config.kind)
    setBaseUrl(config.base_url)
    setModel(config.model)
    setEmbedModel(config.embed_model ?? '')
    setParams(config.params ?? NO_PARAMS)
    setApiKey('')
  }, [config])

  const settings = (): LlmSettings => ({ kind, base_url: baseUrl.trim(), model: model.trim(), embed_model: embedModel.trim() || null, params })
  const refresh = () => void queryClient.invalidateQueries({ queryKey: queryKeys.models })
  const onError = (failure: unknown) => toast.error(errorMessage(failure))

  // A blank key field sends null, which keeps the stored key.
  const save = useMutation({
    mutationFn: () => api.setLlmConfig(settings(), apiKey || null),
    onSuccess: () => {
      setTestResult(null)
      toast.success('Provider saved')
      refresh()
    },
    onError,
  })
  const clearKey = useMutation({ mutationFn: () => api.setLlmConfig(settings(), ''), onSuccess: refresh, onError })
  const test = useMutation({
    mutationFn: api.testLlm,
    onSuccess: setTestResult,
    onError: (failure) => setTestResult({ ok: false, message: errorMessage(failure) }),
  })

  const loadModels = useMutation({
    mutationFn: () => api.listLlmModels(baseUrl.trim()),
    onSuccess: (names) => {
      setInstalled(names)
      if (names.length === 0) toast.error('Ollama is running but has no models. Run "ollama pull <model>" first.')
      else if (!names.includes(model)) setModel(names[0])
    },
    onError,
  })

  const setParam = (key: keyof ModelParams) => (value: number | null) => setParams((current) => ({ ...current, [key]: value }))
  const remote = kind !== 'ollama' && kind !== 'compat'
  const changeKind = (next: LlmKind) => {
    setKind(next)
    setBaseUrl(KINDS.find((k) => k.kind === next)?.baseUrl ?? '')
    setInstalled([])
    setTestResult(null)
  }

  return (
    <div className="space-y-4">
      <Tabs defaultValue="basic">
        <TabsList>
          <TabsTrigger value="basic">Basic</TabsTrigger>
          <TabsTrigger value="advanced">Advanced</TabsTrigger>
        </TabsList>
        <TabsContent value="basic" className="mt-4">
        <div className="grid gap-4 sm:grid-cols-2 [&>*]:min-w-0">
        <FormField label="Provider" htmlFor="llm-kind">
          <Select value={kind} onValueChange={(value) => changeKind(value as LlmKind)}>
            <SelectTrigger id="llm-kind">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {KINDS.map((k) => (
                <SelectItem key={k.kind} value={k.kind}>
                  {k.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </FormField>
        <FormField label="Base URL" htmlFor="llm-base-url">
          <Input id="llm-base-url" value={baseUrl} onChange={(event) => setBaseUrl(event.target.value)} spellCheck={false} />
        </FormField>
        <FormField label="Model" htmlFor="llm-model">
          <div className="flex min-w-0 gap-2">
            {installed.length > 0 ? (
              <Select value={model} onValueChange={setModel}>
                <SelectTrigger id="llm-model" className="min-w-0">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {installed.map((name) => (
                    <SelectItem key={name} value={name}>
                      {name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            ) : (
              <Input id="llm-model" className="min-w-0" value={model} onChange={(event) => setModel(event.target.value)} spellCheck={false} />
            )}
            {kind === 'ollama' && (
              <Button type="button" variant="outline" className="shrink-0" loading={loadModels.isPending} onClick={() => loadModels.mutate()}>
                Load models
              </Button>
            )}
          </div>
        </FormField>
        <FormField label="Embedding model (optional)" htmlFor="llm-embed">
          <Input id="llm-embed" value={embedModel} onChange={(event) => setEmbedModel(event.target.value)} spellCheck={false} />
        </FormField>
        {kind !== 'ollama' && (
          <FormField
            label="API key"
            htmlFor="llm-key"
            hint={config?.has_key ? 'Key saved. Leave blank to keep it, or type a new one to replace it.' : 'No key saved yet.'}
          >
            <div className="flex min-w-0 gap-2">
              <Input id="llm-key" className="min-w-0" type="password" autoComplete="off" value={apiKey} onChange={(event) => setApiKey(event.target.value)} />
              {config?.has_key && (
                <Button type="button" variant="outline" className="shrink-0" loading={clearKey.isPending} onClick={() => clearKey.mutate()}>
                  Remove key
                </Button>
              )}
            </div>
          </FormField>
        )}
      </div>
        </TabsContent>
        <TabsContent value="advanced" className="mt-4">
          <div className="grid gap-4 sm:grid-cols-2 [&>*]:min-w-0">
            <ParamField id="llm-temp" label="Temperature" hint="0 is precise, 1 is varied. Blank keeps each task's own value (SQL is low, narration is higher)." value={params.temperature} onChange={setParam('temperature')} />
            <ParamField id="llm-top-p" label="Top P" hint="Nucleus sampling, 0 to 1. Blank uses the model default." value={params.top_p} onChange={setParam('top_p')} />
            {kind === 'ollama' && (
              <ParamField id="llm-ctx" label="Context size (tokens)" hint="How much of the schema and history the model can see. Larger uses more memory. Blank uses the Ollama default." value={params.num_ctx} onChange={setParam('num_ctx')} />
            )}
            <ParamField id="llm-timeout" label="Timeout (seconds)" hint="How long to wait for one answer. 10 to 3600." value={params.timeout_secs} onChange={setParam('timeout_secs')} />
          </div>
        </TabsContent>
      </Tabs>
      <p className="text-app-caption text-muted-foreground">
        {remote
          ? 'Your questions, table schema, sample rows and query results are sent to this provider.'
          : kind === 'ollama'
            ? 'Requests go to Ollama on this computer.'
            : 'Questions, schema, sample rows and results are sent to the server at this URL.'}
      </p>
      <div className="flex flex-wrap items-center gap-2">
        <Button loading={save.isPending} disabled={!model.trim() || !baseUrl.trim()} onClick={() => save.mutate()}>
          Save
        </Button>
        <Button variant="outline" loading={test.isPending} onClick={() => test.mutate()}>
          Test connection
        </Button>
        {testResult && (
          <p role="status" className={`min-w-0 max-w-full break-words text-app-label ${testResult.ok ? 'text-success' : 'text-destructive'}`}>
            {testResult.ok ? 'Works: ' : 'Failed: '}
            {testResult.message}
          </p>
        )}
      </div>
      <p className="text-app-caption text-muted-foreground">Test uses the saved settings, so save first.</p>
    </div>
  )
}
