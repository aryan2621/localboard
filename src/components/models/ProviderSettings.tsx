import { useEffect, useState } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { toast } from 'sonner'
import { api, errorMessage, type LlmKind, type LlmSettings } from '../../lib/api'
import { queryClient, queryKeys } from '../../lib/queryClient'
import { Button } from '../ui/button'
import { Input } from '../ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '../ui/select'
import { FormField } from '../layout/ui'

const KINDS: { kind: LlmKind; label: string; baseUrl: string }[] = [
  { kind: 'anthropic', label: 'Anthropic', baseUrl: 'https://api.anthropic.com' },
  { kind: 'openai', label: 'OpenAI', baseUrl: 'https://api.openai.com/v1' },
  { kind: 'gemini', label: 'Google Gemini', baseUrl: 'https://generativelanguage.googleapis.com/v1beta/openai' },
  { kind: 'ollama', label: 'Ollama (this computer)', baseUrl: 'http://localhost:11434/v1' },
  { kind: 'compat', label: 'OpenAI-compatible server', baseUrl: '' },
]

/** Provider, model and key for the AI analyst. Shared by Settings and Onboarding. */
export function ProviderSettings() {
  const { data: config } = useQuery({ queryKey: queryKeys.models, queryFn: api.llmConfig })
  const [kind, setKind] = useState<LlmKind>('anthropic')
  const [baseUrl, setBaseUrl] = useState(KINDS[0].baseUrl)
  const [model, setModel] = useState('')
  const [embedModel, setEmbedModel] = useState('')
  const [apiKey, setApiKey] = useState('')
  const [installed, setInstalled] = useState<string[]>([])
  const [testResult, setTestResult] = useState<{ ok: boolean; message: string } | null>(null)

  useEffect(() => {
    if (!config) return
    setKind(config.kind)
    setBaseUrl(config.base_url)
    setModel(config.model)
    setEmbedModel(config.embed_model ?? '')
    setApiKey('')
  }, [config])

  const settings = (): LlmSettings => ({ kind, base_url: baseUrl.trim(), model: model.trim(), embed_model: embedModel.trim() || null })
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

  const remote = kind !== 'ollama' && kind !== 'compat'
  const changeKind = (next: LlmKind) => {
    setKind(next)
    setBaseUrl(KINDS.find((k) => k.kind === next)?.baseUrl ?? '')
    setInstalled([])
    setTestResult(null)
  }

  return (
    <div className="space-y-4">
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
