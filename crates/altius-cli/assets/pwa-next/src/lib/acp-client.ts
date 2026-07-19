import {
  type Run,
  type CreateRunRequest,
  type ResumeRunRequest,
  type SseEvent,
  type AgentCard,
  type Message as AcpMessage,
} from './types'

export { Run, CreateRunRequest, ResumeRunRequest, SseEvent, AgentCard }
export { AcpMessage as Message }

function headers(token: string): HeadersInit {
  const h: Record<string, string> = {
    'Content-Type': 'application/json',
  }
  if (token) h['Authorization'] = `Bearer ${token}`
  return h
}

export class AcpClient {
  constructor(private baseUrl: string, private token: string = '') {}

  async listRuns(): Promise<Run[]> {
    const res = await fetch(`${this.baseUrl}/runs`, {
      headers: headers(this.token),
    })
    if (!res.ok) throw new Error(`listRuns: ${res.status}`)
    return res.json()
  }

  async getRun(id: string): Promise<Run> {
    const res = await fetch(`${this.baseUrl}/runs/${id}`, {
      headers: headers(this.token),
    })
    if (!res.ok) throw new Error(`getRun: ${res.status}`)
    return res.json()
  }

  async createRun(agentName: string, input: AcpMessage[]): Promise<Run> {
    const body: CreateRunRequest = { agent_name: agentName, input }
    const res = await fetch(`${this.baseUrl}/runs`, {
      method: 'POST',
      headers: headers(this.token),
      body: JSON.stringify(body),
    })
    if (!res.ok) throw new Error(`createRun: ${res.status} ${await res.text()}`)
    return res.json()
  }

  async resumeRun(id: string, req: ResumeRunRequest): Promise<Run> {
    const res = await fetch(`${this.baseUrl}/runs/${id}`, {
      method: 'POST',
      headers: headers(this.token),
      body: JSON.stringify(req),
    })
    if (!res.ok) throw new Error(`resumeRun: ${res.status}`)
    return res.json()
  }

  async cancelRun(id: string): Promise<Run> {
    const res = await fetch(`${this.baseUrl}/runs/${id}/cancel`, {
      method: 'POST',
      headers: headers(this.token),
    })
    if (!res.ok) throw new Error(`cancelRun: ${res.status}`)
    return res.json()
  }

  async getAgentCard(): Promise<AgentCard> {
    const res = await fetch(`${this.baseUrl}/.well-known/agent-card.json`, {
      headers: headers(this.token),
    })
    if (!res.ok) throw new Error(`getAgentCard: ${res.status}`)
    return res.json()
  }

  async *streamEvents(runId: string): AsyncGenerator<SseEvent> {
    const url = new URL(`${this.baseUrl}/runs/${runId}/events`)
    if (this.token) url.searchParams.set('token', this.token)

    const res = await fetch(url, {
      headers: { Accept: 'text/event-stream' },
    })
    if (!res.ok || !res.body) throw new Error(`streamEvents: ${res.status}`)

    const reader = res.body.getReader()
    const decoder = new TextDecoder()
    let buffer = ''
    let currentEvent = ''
    let currentData = ''

    while (true) {
      const { done, value } = await reader.read()
      if (done) break

      buffer += decoder.decode(value, { stream: true })
      const lines = buffer.split('\n')
      buffer = lines.pop() || ''

      for (const line of lines) {
        if (line.startsWith('event: ')) {
          currentEvent = line.slice(7).trim()
        } else if (line.startsWith('data: ')) {
          currentData = line.slice(6)
        } else if (line === '' && currentData) {
          if (currentEvent === 'run') {
            try {
              const run = JSON.parse(currentData) as Run
              yield { type: 'run', run }
            } catch {
              // skip malformed
            }
          }
          currentEvent = ''
          currentData = ''
        }
      }
    }
  }
}

// Message factory helpers
export function userText(content: string): AcpMessage {
  return { role: 'user', parts: [{ content_type: 'text/plain', content }] }
}

export function agentText(content: string): AcpMessage {
  return { role: 'agent', parts: [{ content_type: 'text/plain', content }] }
}
