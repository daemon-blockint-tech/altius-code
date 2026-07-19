'use client'

import { useState, useMemo } from 'react'
import type { Run } from '@/lib/types'
import { partsText, formatTime, truncate, cn } from '@/lib/utils'
import { CheckCircle2, XCircle, Clock, Loader2, AlertCircle, Search } from 'lucide-react'

const statusConfig: Record<string, { icon: typeof CheckCircle2; color: string; label: string }> = {
  created: { icon: Clock, color: 'text-text-secondary', label: 'Created' },
  'in-progress': { icon: Loader2, color: 'text-accent', label: 'Running' },
  awaiting: { icon: AlertCircle, color: 'text-warn', label: 'Awaiting' },
  completed: { icon: CheckCircle2, color: 'text-ok', label: 'Done' },
  failed: { icon: XCircle, color: 'text-bad', label: 'Failed' },
  cancelled: { icon: XCircle, color: 'text-text-secondary', label: 'Cancelled' },
}

const filterOptions = [
  { key: 'all', label: 'All' },
  { key: 'active', label: 'Active' },
  { key: 'done', label: 'Done' },
  { key: 'failed', label: 'Failed' },
] as const

type FilterKey = typeof filterOptions[number]['key']

function matchesFilter(run: Run, filter: FilterKey): boolean {
  if (filter === 'all') return true
  if (filter === 'active') return ['created', 'in-progress', 'awaiting'].includes(run.status)
  if (filter === 'done') return run.status === 'completed'
  if (filter === 'failed') return run.status === 'failed' || run.status === 'cancelled'
  return true
}

function formatDuration(start: string, end?: string): string {
  if (!end) return ''
  const ms = new Date(end).getTime() - new Date(start).getTime()
  if (ms < 1000) return `${ms}ms`
  if (ms < 60000) return `${(ms / 1000).toFixed(1)}s`
  return `${(ms / 60000).toFixed(1)}m`
}

interface TaskSidebarProps {
  runs: Run[]
  activeRunId: string | null
  onSelect: (run: Run) => void
  loading?: boolean
  className?: string
}

export function TaskSidebar({ runs, activeRunId, onSelect, loading, className }: TaskSidebarProps) {
  const [search, setSearch] = useState('')
  const [filter, setFilter] = useState<FilterKey>('all')

  const filtered = useMemo(() => {
    return runs.filter((run) => {
      if (!matchesFilter(run, filter)) return false
      if (search.trim()) {
        const prompt = partsText(run.input.flatMap((m) => m.parts))
        return prompt.toLowerCase().includes(search.toLowerCase()) || run.agent_name.toLowerCase().includes(search.toLowerCase())
      }
      return true
    })
  }, [runs, filter, search])

  return (
    <aside className={cn('w-64 border-r border-border-subtle bg-bg-elevated overflow-y-auto flex-shrink-0 flex flex-col', className)}>
      <div className="p-3 flex-shrink-0">
        <div className="flex items-center justify-between mb-3">
          <h2 className="font-display text-xs font-semibold uppercase tracking-wide text-text-secondary">
            Runs
          </h2>
          {runs.length > 0 && (
            <span className="text-xs text-text-tertiary font-mono">{runs.length}</span>
          )}
        </div>

        {/* Search */}
        {runs.length > 0 && (
          <div className="relative mb-2">
            <Search className="absolute left-2 top-1/2 -translate-y-1/2 w-3 h-3 text-text-tertiary" />
            <input
              type="text"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder="Search..."
              className="w-full pl-7 pr-2 py-1 text-xs rounded border border-border-subtle bg-bg-primary text-text-primary placeholder:text-text-tertiary focus:outline-none focus:border-accent"
            />
          </div>
        )}

        {/* Filter buttons */}
        {runs.length > 0 && (
          <div className="flex gap-1 mb-2">
            {filterOptions.map((opt) => (
              <button
                key={opt.key}
                onClick={() => setFilter(opt.key)}
                className={cn(
                  'px-1.5 py-0.5 rounded text-[10px] font-medium transition-colors',
                  filter === opt.key
                    ? 'bg-accent text-white'
                    : 'bg-bg-secondary text-text-secondary hover:bg-bg-primary',
                )}
              >
                {opt.label}
              </button>
            ))}
          </div>
        )}
      </div>

      <div className="flex-1 overflow-y-auto px-3 pb-3">
        {loading ? (
          <div className="flex items-center justify-center py-8">
            <Loader2 className="w-4 h-4 animate-spin text-text-tertiary" />
          </div>
        ) : filtered.length === 0 ? (
          <p className="text-sm text-text-tertiary py-4 text-center">
            {runs.length === 0 ? 'No runs yet' : 'No matches'}
          </p>
        ) : (
          <ul className="space-y-1">
            {filtered.map((run) => {
              const cfg = statusConfig[run.status] || statusConfig.created
              const Icon = cfg.icon
              const prompt = partsText(run.input.flatMap((m) => m.parts))
              const duration = formatDuration(run.created_at, run.finished_at)
              return (
                <li key={run.run_id}>
                  <button
                    onClick={() => onSelect(run)}
                    className={cn(
                      'w-full text-left px-3 py-2 rounded-md transition-colors text-sm',
                      'hover:bg-bg-secondary',
                      activeRunId === run.run_id && 'bg-accent-soft',
                    )}
                  >
                    <div className="flex items-center gap-2 mb-0.5">
                      <Icon
                        className={cn(
                          'w-3.5 h-3.5 flex-shrink-0',
                          cfg.color,
                          run.status === 'in-progress' && 'animate-spin',
                        )}
                      />
                      <span className="text-xs text-text-secondary">
                        {formatTime(run.created_at)}
                      </span>
                      {duration && (
                        <span className="text-[10px] text-text-tertiary ml-auto">{duration}</span>
                      )}
                    </div>
                    <p className="text-text-primary text-xs leading-snug">
                      {truncate(prompt || run.agent_name, 40)}
                    </p>
                  </button>
                </li>
              )
            })}
          </ul>
        )}
      </div>
    </aside>
  )
}
