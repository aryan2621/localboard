import { useEffect, useMemo, useRef, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { api, type SearchHit } from '../../lib/api'
import { useNavigation, type AppPage } from '../../stores/navigation'
import { Dialog, DialogContent, DialogDescription, DialogTitle } from '../ui/dialog'

const ACTIONS: (SearchHit & { titleKey: string; subtitleKey?: string })[] = [
  { kind: 'page', id: 'home', title: 'Home', titleKey: 'Home', subtitle: null, subtitleKey: 'Go to Home' },
  { kind: 'page', id: 'chat', title: 'New chat', titleKey: 'New chat', subtitle: null, subtitleKey: 'Go to Chat' },
  { kind: 'page', id: 'history', title: 'Chat history', titleKey: 'Chat history', subtitle: null },
  { kind: 'page', id: 'connections', title: 'Connections', titleKey: 'Connections', subtitle: null },
  { kind: 'page', id: 'schema', title: 'Schema', titleKey: 'Schema', subtitle: null },
  { kind: 'page', id: 'bench', title: 'Query Bench', titleKey: 'Query Bench', subtitle: null },
  { kind: 'page', id: 'dashboards', title: 'Dashboards', titleKey: 'Dashboards', subtitle: null },
  { kind: 'page', id: 'automations', title: 'Automations', titleKey: 'Automations', subtitle: null },
  { kind: 'page', id: 'datasites', title: 'Publish', titleKey: 'Publish', subtitle: null },
  { kind: 'page', id: 'agents', title: 'Analysts', titleKey: 'Analysts', subtitle: null },
  { kind: 'page', id: 'audit', title: 'Audit', titleKey: 'Audit', subtitle: null },
]

function openHit(hit: SearchHit, nav: { navigate: (page: AppPage) => void; openConversation: (id: string) => void; openDashboard: (id: string | null) => void; openDatasite: (id: string | null) => void; openBench: (sql?: string) => void }) {
  switch (hit.kind) {
    case 'page':
      nav.navigate(hit.id as AppPage)
      break
    case 'chat':
      nav.openConversation(hit.id)
      break
    case 'dashboard':
      nav.openDashboard(hit.id)
      break
    case 'datasite':
      nav.openDatasite(hit.id)
      break
    case 'automation':
      nav.navigate('automations')
      break
    case 'query':
      nav.openBench(hit.subtitle ?? undefined)
      break
    default:
      break
  }
}

export function CommandPalette() {
  const navigate = useNavigation()
  const returnFocus = useRef<HTMLElement | null>(null)
  const [open, setOpen] = useState(false)
  const [query, setQuery] = useState('')
  const [active, setActive] = useState(0)
  const search = useQuery({
    queryKey: ['palette', query],
    queryFn: () => api.searchWorkspace(query),
    enabled: open && query.trim().length > 0,
  })

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
        event.preventDefault()
        setOpen((current) => {
          if (!current) returnFocus.current = document.activeElement instanceof HTMLElement ? document.activeElement : null
          return !current
        })
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  const hits = useMemo((): (SearchHit | (typeof ACTIONS)[number])[] => {
    const remote = search.data ?? []
    const pages = ACTIONS.filter((item) => !query.trim() || item.titleKey.toLowerCase().includes(query.trim().toLowerCase()))
    return [...remote, ...pages].slice(0, 40)
  }, [query, search.data])

  useEffect(() => {
    setActive(0)
  }, [query, hits.length])

  const choose = (hit: SearchHit) => {
    openHit(hit, navigate)
    setOpen(false)
    setQuery('')
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        setOpen(next)
        if (!next) setQuery('')
      }}
    >
      <DialogContent
        className="max-w-xl gap-2 p-3"
        onCloseAutoFocus={(event) => {
          event.preventDefault()
          returnFocus.current?.focus()
        }}
      >
        <DialogTitle className="sr-only">{'Command palette'}</DialogTitle>
        <DialogDescription className="sr-only">{'Search chats, dashboards, datasites, apps, automations and saved queries.'}</DialogDescription>
        <input
          autoFocus
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'ArrowDown') {
              event.preventDefault()
              setActive((index) => Math.min(index + 1, Math.max(hits.length - 1, 0)))
            } else if (event.key === 'ArrowUp') {
              event.preventDefault()
              setActive((index) => Math.max(index - 1, 0))
            } else if (event.key === 'Enter' && hits[active]) {
              event.preventDefault()
              choose(hits[active])
            }
          }}
          placeholder={'Search or jump…'}
          aria-label={'Search LocalBoard'}
          className="h-10 w-full rounded-lg border border-input bg-popover px-3 text-app-label focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40"
        />
        <ul className="max-h-80 overflow-y-auto">
          {hits.map((hit, index) => (
            <li key={`${hit.kind}-${hit.id}-${index}`}>
              <button
                type="button"
                onClick={() => choose(hit)}
                className={`flex w-full flex-col rounded-lg px-3 py-2 text-left text-app-label focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/40 ${index === active ? 'bg-accent' : ''}`}
              >
                <span>{'titleKey' in hit ? hit.titleKey : hit.title}</span>
                <span className="text-app-caption text-muted-foreground">{'subtitleKey' in hit && hit.subtitleKey ? hit.subtitleKey : hit.kind}</span>
              </button>
            </li>
          ))}
        </ul>
      </DialogContent>
    </Dialog>
  )
}
