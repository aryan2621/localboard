import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { Database, LayoutDashboard, MessageSquare, FileText, Rows3, Table2 } from 'lucide-react'
import { ChoiceGrid, StatRow } from '../../components/layout/patterns'
import { ListItemCard } from '../../components/layout/ui'
import { GenerationSteps } from '../../components/analyst/StepsPanel'
import { Composer, type OutputKind } from '../../components/analyst/Composer'
import { Button } from '../../components/ui/button'
import { EmptyState } from '../../components/layout/ui'
import { useGenerate } from '../../lib/analyst'
import { api } from '../../lib/api'
import { queryKeys } from '../../lib/queryClient'
import { useNavigation } from '../../stores/navigation'

const STARTS: { value: string; title: string; description: string; icon: typeof Database; output: OutputKind; prompt: string }[] = [
  { value: 'ask', title: 'Ask a question', description: 'Get an answer with a chart and the SQL behind it.', icon: MessageSquare, output: 'answer', prompt: 'What changed in orders last month, and why?' },
  { value: 'dashboard', title: 'Build a dashboard', description: 'A set of charts you can filter and refresh.', icon: LayoutDashboard, output: 'dashboard', prompt: 'Monthly orders, revenue and refunds by region' },
  { value: 'report', title: 'Write a report', description: 'A PDF with findings, charts and tables.', icon: FileText, output: 'report', prompt: 'A short report on last month’s orders and refunds' },
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

  return (
    <div className="mx-auto flex max-w-3xl flex-col px-6 pb-10 pt-[12vh] md:px-8">
      <header className="space-y-2 pb-8 text-center">
        <h1 className="font-serif text-app-display text-foreground">{'Ask about your business'}</h1>
        <p className="text-app-label text-muted-foreground">{'Answers are computed from your data on this machine. Nothing leaves it.'}</p>
      </header>

      {tables && !hasData ? (
        <EmptyState
          icon={Database}
          title="No data yet"
          description="Add a file, folder or database first. Then ask anything about it."
          action={<Button onClick={() => navigate('connections')}>{'Add data'}</Button>}
        />
      ) : (
        <div className="space-y-5">
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
          {!generate.isPending && (
            <section className="space-y-3 pt-6">
              <h2 className="text-app-caption font-medium text-muted-foreground">Start from</h2>
              <ChoiceGrid
                columns={3}
                value={null}
                onSelect={(choice) => {
                  const start = STARTS.find((item) => item.value === choice)
                  if (!start) return
                  setPreset(start.output)
                  setComposerText(start.prompt)
                }}
                options={STARTS.map((item) => ({ value: item.value, title: item.title, description: item.description, icon: item.icon }))}
              />
            </section>
          )}
          {!generate.isPending && tables && tables.length > 0 && (
            <section className="space-y-3">
              <h2 className="text-app-caption font-medium text-muted-foreground">Your data</h2>
              <StatRow
                stats={[
                  { label: 'Sources', value: sources.length, icon: Database, onClick: () => navigate('connections') },
                  { label: 'Tables', value: tables.length, icon: Table2, onClick: () => navigate('schema') },
                  {
                    label: 'Rows',
                    value: new Intl.NumberFormat(undefined, { notation: 'compact' }).format(tables.reduce((sum, table) => sum + (table.row_count ?? 0), 0)),
                    icon: Rows3,
                    onClick: () => navigate('schema'),
                  },
                ]}
              />
            </section>
          )}
          {!generate.isPending && (chats.length > 0 || boards.length > 0) && (
            <div className="grid animate-rise-in gap-6 pt-6 sm:grid-cols-2">
              {chats.length > 0 && (
                <section className="space-y-2">
                  <h2 className="text-app-caption font-medium text-muted-foreground">Recent chats</h2>
                  {chats.slice(0, 4).map((chat) => (
                    <ListItemCard key={chat.id} title={chat.title} onClick={() => openConversation(chat.id)} />
                  ))}
                </section>
              )}
              {boards.length > 0 && (
                <section className="space-y-2">
                  <h2 className="text-app-caption font-medium text-muted-foreground">Dashboards</h2>
                  {boards.slice(0, 4).map((board) => (
                    <ListItemCard
                      key={board.id}
                      title={board.name}
                      meta={`${board.tileCount} ${board.tileCount === 1 ? 'tile' : 'tiles'}`}
                      onClick={() => openDashboard(board.id)}
                    />
                  ))}
                </section>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  )
}
