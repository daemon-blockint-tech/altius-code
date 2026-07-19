export type RunStatus =
  | 'created'
  | 'in-progress'
  | 'awaiting'
  | 'completed'
  | 'failed'
  | 'cancelled'

export interface MessagePart {
  content_type: string
  content: string
}

export interface Message {
  role: string
  parts: MessagePart[]
}

export interface RunApproval {
  summary: string
  reason?: string
  node?: string
  kind: 'generic' | 'transaction'
  transaction?: {
    action_summary?: string
    lamport_deltas?: { account: string; delta_lamports: string }[]
    invoked_programs?: string[]
    compute_units_consumed?: number
    compute_unit_limit?: number
  }
}

export interface Run {
  run_id: string
  agent_name: string
  status: RunStatus
  input: Message[]
  output: Message[]
  error?: string
  approval?: RunApproval
  created_at: string
  finished_at?: string
}

export interface CreateRunRequest {
  agent_name: string
  input: Message[]
}

export interface ResumeRunRequest {
  message?: Message
  decision?: { approved: boolean; note?: string }
}

export interface SseEvent {
  type: 'run' | 'message'
  run?: Run
  message?: Message
}
