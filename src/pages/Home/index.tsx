import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { GenerationSteps } from '../../components/analyst/StepsPanel'
import { Composer, type OutputKind } from '../../components/analyst/Composer'
import { Button } from '../../components/ui/button'
import { EmptyState } from '../../components/layout/ui'
import { useGenerate } from '../../lib/analyst'
import { api } from '../../lib/api'
import { queryKeys } from '../../lib/queryClient'
import { useNavigation } from '../../stores/navigation'

const STARTS: { title: string; output: OutputKind; prompt: string }[] = [
  { title: 'Ask a question', output: 'answer', prompt: 'What changed in orders last month, and why?' },
  { title: 'Build a dashboard', output: 'dashboard', prompt: 'Monthly orders, revenue and refunds by region' },
  { title: 'Write a report', output: 'report', prompt: 'A short report on last month’s orders and refunds' },
]

export function HomePage() {
  const [composerText, setComposerText] = useState('')
  const [preset, setPreset] = useState<OutputKind | undefined>(undefined)

  const { data: tables } = useQuery({ queryKey: queryKeys.tables, queryFn: api.tables })
  const hasData = Boolean(tables && tables.length > 0)

  const navigate = useNavigation((state) => state.navigate)
  const generate = useGenerate()

  const submit = (question: string, output: OutputKind) => generate.mutateAsync({ question, conversationId: null, output })

  return (
    <div className="mx-auto flex min-h-full max-w-[760px] flex-col justify-center gap-8 px-6 py-12 md:px-8">
      <header className="space-y-2 text-center">
        <h1 className="text-app-display font-semibold text-foreground">What do you want to know?</h1>
        <p className="text-app-label text-muted-foreground">Answers are computed on this machine. Nothing leaves it.</p>
      </header>

      {tables && !hasData ? (
        <EmptyState
          title="No data yet"
          description="Add a file, folder or database first. Then ask anything about it."
          action={<Button onClick={() => navigate('connections')}>{'Add data'}</Button>}
        />
      ) : (
        <>
          <div className="space-y-3">
            <Composer
              onSubmit={submit}
              busy={generate.isPending}
              disabled={!hasData}
              autoFocus
              draftValue={composerText}
              onDraftChange={setComposerText}
              chips
              outputValue={preset}
            />
            <GenerationSteps running={generate.isPending} />
          </div>
          {!generate.isPending && (
            <div className="flex flex-wrap justify-center gap-2">
              {STARTS.map((start) => (
                <button
                  key={start.title}
                  type="button"
                  title={start.prompt}
                  onClick={() => {
                    setPreset(start.output)
                    setComposerText(start.prompt)
                  }}
                  className="rounded-full border border-border px-3.5 py-1.5 text-app-label text-foreground transition-colors duration-fast hover:bg-accent focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                >
                  {start.title}
                </button>
              ))}
            </div>
          )}
        </>
      )}
    </div>
  )
}
