'use client'

import { cn } from '@/lib/utils'

interface StatusBarProps {
  health: 'ok' | 'down' | 'checking'
  apiBase: string
  agentCount?: number
}

export function StatusBar({ health, apiBase, agentCount }: StatusBarProps) {
  const dotColor = {
    ok: 'bg-ok',
    down: 'bg-bad',
    checking: 'bg-warn',
  }[health]

  const label = {
    ok: 'Connected',
    down: 'Disconnected',
    checking: 'Connecting...',
  }[health]

  return (
    <footer className="flex items-center gap-3 border-t border-border-subtle bg-bg-elevated px-4 py-1.5 text-xs text-text-secondary">
      <div className="flex items-center gap-1.5">
        <span className={cn('w-2 h-2 rounded-full', dotColor)} />
        <span>{label}</span>
      </div>
      {apiBase && (
        <>
          <span className="text-text-tertiary">|</span>
          <span className="font-mono">{apiBase}</span>
        </>
      )}
      {agentCount != null && agentCount > 0 && (
        <>
          <span className="text-text-tertiary">|</span>
          <span>{agentCount} agent{agentCount !== 1 ? 's' : ''}</span>
        </>
      )}
      <span className="ml-auto text-text-tertiary">Altius Fleet</span>
    </footer>
  )
}
