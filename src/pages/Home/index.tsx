import { useState, type ReactNode } from 'react'
import { useQuery } from '@tanstack/react-query'
import { Database } from 'lucide-react'
import { ListItemCard } from '../../components/layout/ui'
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
  const openConversation = useNavigation((state) => state.openConversation)
  const openDashboard = useNavigation((state) => state.openDashboard)
  const { data: sources = [] } = useQuery({ queryKey: queryKeys.connections, queryFn: api.connections, enabled: hasData })
  const { data: chats = [] } = useQuery({ queryKey: queryKeys.conversationSearch(''), queryFn: () => api.conversations(''), enabled: hasData })
  const { data: boards = [] } = useQuery({ queryKey: queryKeys.dashboards, queryFn: api.dashboards, enabled: hasData })
  const generate = useGenerate()

  const submit = (question: string, output: OutputKind) => generate.mutateAsync({ question, conversationId: null, output })

  const rowCount = tables?.reduce((sum, table) => sum + (table.row_count ?? 0), 0) ?? 0
  const fmt = new Intl.NumberFormat(undefined, { notation: 'compact' })

  return (
    <div className="mx-auto flex max-w-[720px] flex-col gap-10 px-6 pb-12 pt-[10vh] md:px-8">
      <header className="space-y-1">
        <h1 className="text-app-title font-semibold text-foreground">Ask about your data</h1>
        <p className="text-app-label text-muted-foreground">Answers are computed on this machine. Nothing leaves it.</p>
      </header>

      {tables && !hasData ? (
        <EmptyState
          icon={Database}
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
            <HomeSection title="Try">
              {STARTS.map((start) => (
                <ListItemCard
                  key={start.title}
                  title={start.title}
                  meta={start.prompt}
                  onClick={() => {
                    setPreset(start.output)
                    setComposerText(start.prompt)
                  }}
                />
              ))}
            </HomeSection>
          )}
          {!generate.isPending && chats.length > 0 && (
            <HomeSection title="Recent chats" action="View all" onAction={() => navigate('history')}>
              {chats.slice(0, 5).map((chat) => (
                <ListItemCard key={chat.id} title={chat.title} onClick={() => openConversation(chat.id)} />
              ))}
            </HomeSection>
          )}
          {!generate.isPending && boards.length > 0 && (
            <HomeSection title="Dashboards" action="View all" onAction={() => navigate('dashboards')}>
              {boards.slice(0, 5).map((board) => (
                <ListItemCard
                  key={board.id}
                  title={board.name}
                  meta={`${board.tileCount} ${board.tileCount === 1 ? 'tile' : 'tiles'}`}
                  onClick={() => openDashboard(board.id)}
                />
              ))}
            </HomeSection>
          )}
          {!generate.isPending && tables && tables.length > 0 && (
            <HomeSection
              title={`Data: ${sources.length} ${sources.length === 1 ? 'source' : 'sources'}, ${tables.length} tables, ${fmt.format(rowCount)} rows`}
              action="Schema"
              onAction={() => navigate('schema')}
            >
              {tables.slice(0, 5).map((table) => (
                <ListItemCard key={table.id} title={table.relation} meta={`${fmt.format(table.row_count ?? 0)} rows`} onClick={() => navigate('schema')} />
              ))}
            </HomeSection>
          )}
        </>
      )}
    </div>
  )
}

function HomeSection({ title, action, onAction, children }: { title: string; action?: string; onAction?: () => void; children: ReactNode }) {
  return (
    <section>
      <div className="flex items-center justify-between border-b border-border px-2 pb-1.5">
        <h2 className="text-app-caption font-medium text-muted-foreground">{title}</h2>
        {action && (
          <button
            type="button"
            onClick={onAction}
            className="rounded-md text-app-caption text-muted-foreground transition-colors duration-fast hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            {action}
          </button>
        )}
      </div>
      <div className="pt-1">{children}</div>
    </section>
  )
}
