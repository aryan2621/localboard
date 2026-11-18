import { useEffect, useRef, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { api, type SearchHit } from '../../lib/api'
import { useNavigation, type AppPage } from '../../stores/navigation'
import { Command, CommandEmpty, CommandInput, CommandItem, CommandList } from '../ui/command'
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

  const hits: (SearchHit | (typeof ACTIONS)[number])[] = [...(search.data ?? []), ...ACTIONS.filter((item) => !query.trim() || item.titleKey.toLowerCase().includes(query.trim().toLowerCase()))].slice(0, 40)

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
        className="max-w-xl gap-0 overflow-hidden p-0"
        onCloseAutoFocus={(event) => {
          event.preventDefault()
          returnFocus.current?.focus()
        }}
      >
        <DialogTitle className="sr-only">{'Command palette'}</DialogTitle>
        <DialogDescription className="sr-only">{'Search chats, dashboards, datasites, apps, automations and saved queries.'}</DialogDescription>
        <Command shouldFilter={false} loop>
          <CommandInput value={query} onValueChange={setQuery} placeholder="Search or jump…" aria-label="Search LocalBoard" autoFocus />
          <CommandList>
            <CommandEmpty>No results</CommandEmpty>
            {hits.map((hit, index) => (
              <CommandItem key={`${hit.kind}-${hit.id}-${index}`} value={`${hit.kind}-${hit.id}-${index}`} onSelect={() => choose(hit)}>
                <span className="truncate">{'titleKey' in hit ? hit.titleKey : hit.title}</span>
                <span className="truncate text-app-caption text-muted-foreground">{'subtitleKey' in hit && hit.subtitleKey ? hit.subtitleKey : hit.kind}</span>
              </CommandItem>
            ))}
          </CommandList>
        </Command>
      </DialogContent>
    </Dialog>
  )
}
