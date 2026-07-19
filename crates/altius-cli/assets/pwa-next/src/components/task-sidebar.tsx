'use client'

import type { Run } from '@/lib/types'
import { partsText, formatTime, truncate, cn } from '@/lib/utils'
import { CheckCircle2, XCircle, Clock, Loader2, AlertCircle } from 'lucide-react'

const statusConfig: Record<string, { icon: typeof CheckCircle2; color: string; label: string }> = {
  created: { icon: Clock, color: 'text-text-secondary', label: 'Created' },
  'in-progress': { icon: Loader2, color: 'text-accent', label: 'Running' },
  awaiting: { icon: AlertCircle, color: 'text-warn', label: 'Awaiting' },
  completed: { icon: CheckCircle2, color: 'text-ok', label: 'Done' },
  failed: { icon: XCircle, color: 'text-bad', label: 'Failed' },
  cancelled: { icon: XCircle, color: 'text-text-secondary', label: 'Cancelled' },
}

interface TaskSidebarProps {
  runs: Run[]
  activeRunId: string | null
  onSelect: (run: Run) => void
}

export function TaskSidebar({ runs, activeRunId, onSelect }: TaskSidebarProps) {
  return (
    <aside className="w-64 border-r border-border-subtle bg-bg-elevated overflow-y-auto flex-shrink-0">
      <div className="p-3">
        <h2 className="font-display text-xs font-semibold uppercase tracking-wide text-text-secondary mb-3">
          Runs
        </h2>
        {runs.length === 0 ? (
          <p className="text-sm text-text-tertiary py-4 text-center">No runs yet</p>
        ) : (
          <ul className="space-y-1">
            {runs.map((run) => {
              const cfg = statusConfig[run.status] || statusConfig.created
              const Icon = cfg.icon
              const prompt = partsText(run.input.flatMap((m) => m.parts))
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
