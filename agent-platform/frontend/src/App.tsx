import './index.css';
import React, { useState, useEffect, useCallback, useRef } from 'react';

const API = 'http://127.0.0.1:4000';
const WS = 'ws://127.0.0.1:4000/api/ws';

type Org = { id: string; name: string; description: string; created_at: string };
type Agent = { id: string; org_id: string; name: string; role: string; description: string; status: 'idle'|'working'|'completed'|'error'; current_task?: string; created_at: string };
type Task = { id: string; org_id: string; agent_id: string; description: string; status: 'pending'|'running'|'completed'|'failed'; result?: string; created_at: string; completed_at?: string };

export default function App() {
  return (
    <div className="h-screen bg-[#0a0a0f] text-gray-100 flex font-sans">
      <Sidebar />
      <MainPanel />
    </div>
  );
}

// ── Sidebar ──
function Sidebar() {
  const [orgs, setOrgs] = useState<Org[]>([]);
  const [selected, setSelected] = useOrgStore(s => [s.selectedOrg, s.setSelectedOrg]);
  const [showCreate, setShowCreate] = useState(false);

  const load = useCallback(async () => {
    try { const r = await fetch(`${API}/api/orgs`); if (!r.ok) throw new Error(`HTTP ${r.status}`);
    setOrgs(await r.json()); } catch(e) { console.warn('Backend not available:', e); }
  }, []);

  useEffect(() => { load(); }, [load]);

  // Refresh on WS events
  useEffect(() => {
    const ws = new WebSocket(WS);
    ws.onmessage = () => load();
    return () => ws.close();
  }, [load]);

  return (
    <div className="w-64 bg-[#0d0d15] border-r border-gray-800 flex flex-col">
      <div className="p-5 border-b border-gray-800">
        <div className="flex items-center gap-2 mb-1">
          <div className="w-7 h-7 rounded-lg bg-gradient-to-br from-indigo-500 to-purple-600 flex items-center justify-center text-sm font-bold">A</div>
          <span className="font-semibold text-base">Agent Platform</span>
        </div>
        <p className="text-xs text-gray-500 mt-1">Orchestration Dashboard</p>
      </div>

      <div className="flex-1 overflow-y-auto p-3">
        <div className="flex items-center justify-between mb-3">
          <span className="text-xs font-medium text-gray-400 uppercase tracking-wider">Organizations</span>
          <button onClick={() => setShowCreate(true)} className="w-6 h-6 rounded-md bg-indigo-600 hover:bg-indigo-500 text-white flex items-center justify-center text-lg transition-colors">+</button>
        </div>
        {orgs.map(o => (
          <button key={o.id} onClick={() => setSelected(o.id)}
            className={`w-full text-left px-3 py-2.5 rounded-lg mb-1 transition-all duration-150 ${
              selected === o.id ? 'bg-indigo-600/20 border border-indigo-500/30 text-white' : 'hover:bg-gray-800/50 text-gray-400 border border-transparent'
            }`}>
            <div className="text-sm font-medium truncate">{o.name}</div>
            <div className="text-xs text-gray-500 truncate mt-0.5">{o.description}</div>
          </button>
        ))}
        {orgs.length === 0 && <p className="text-xs text-gray-600 text-center mt-8">No organizations yet</p>}
      </div>

      <CreateOrgModal open={showCreate} onClose={() => setShowCreate(false)} />
    </div>
  );
}

// ── Main Panel ──
function MainPanel() {
  const selected = useOrgStore(s => s.selectedOrg);
  const [orgs, setOrgs] = useState<Org[]>([]);

  useEffect(() => {
    fetch(`${API}/api/orgs`).then(r => r.json()).then(setOrgs);
  }, [selected]);

  const org = orgs.find(o => o.id === selected);

  if (!selected || !org) {
    return (
      <div className="flex-1 flex items-center justify-center">
        <div className="text-center">
          <div className="w-16 h-16 rounded-2xl bg-gradient-to-br from-indigo-500 to-purple-600 mx-auto flex items-center justify-center text-2xl font-bold mb-4">A</div>
          <h2 className="text-xl font-semibold mb-2">Agent Platform</h2>
          <p className="text-gray-500 text-sm">Select an organization or create a new one to begin</p>
        </div>
      </div>
    );
  }

  return (
    <div className="flex-1 flex flex-col overflow-hidden">
      <Header org={org} />
      <AgentGrid orgId={selected} />
    </div>
  );
}

// ── Header ──
function Header({ org }: { org: Org }) {
  const [showAgent, setShowAgent] = useState(false);
  const setSelected = useOrgStore(s => s.setSelectedOrg);

  return (
    <div className="px-6 py-4 bg-[#0d0d15] border-b border-gray-800 flex items-center justify-between">
      <div>
        <div className="flex items-center gap-3">
          <button onClick={() => setSelected(null)} className="text-gray-500 hover:text-gray-300 text-sm">&larr; Back</button>
          <h1 className="text-lg font-semibold">{org.name}</h1>
          <span className="px-2 py-0.5 rounded-full text-xs bg-gray-800 text-gray-400">{org.description}</span>
        </div>
      </div>
      <button onClick={() => setShowAgent(true)} className="px-4 py-2 bg-indigo-600 hover:bg-indigo-500 text-white text-sm rounded-lg font-medium transition-colors flex items-center gap-2">
        <span>+</span> Create Agent
      </button>
      <CreateAgentModal orgId={org.id} open={showAgent} onClose={() => setShowAgent(false)} />
    </div>
  );
}

// ── Agent Grid ──
function AgentGrid({ orgId }: { orgId: string }) {
  const [agents, setAgents] = useState<Agent[]>([]);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [events, setEvents] = useState<string[]>([]);
  const [agentProgress, setAgentProgress] = useState<Record<string,string>>({});
  const [search, setSearch] = useState('');
  const [statusFilter, setStatusFilter] = useState('');

  const load = useCallback(async () => {
    const [ar, tr] = await Promise.all([
      fetch(`${API}/api/orgs/${orgId}/agents`).then(r => r.json()),
      fetch(`${API}/api/orgs/${orgId}/agents/_/tasks`).then(r => r.json()),
    ]);
    setAgents(ar);
    setTasks(tr);
  }, [orgId]);

  useEffect(() => { load(); }, [load]);

  useEffect(() => {
    const ws = new WebSocket(WS);
    ws.onmessage = (e) => {
      const msg = e.data;
      setEvents(prev => [...prev.slice(-100), msg]);
      if (msg.startsWith('progress:')) {
        const parts = msg.split(':');
        const agentId = parts[1];
        const action = parts.slice(2).join(':');
        setAgentProgress(prev => ({...prev, [agentId]: action}));
      } else if (msg.startsWith('done:')) {
        setTimeout(() => setAgentProgress(prev => {
          const next = {...prev};
          Object.keys(next).forEach(k => { if (next[k].includes(msg.substring(5,11))) delete next[k]; });
          return next;
        }), 3000);
      }
      load();
    };
    return () => ws.close();
  }, [load]);

  const filteredTasks = tasks.filter(t => {
    const matchSearch = !search || t.description.toLowerCase().includes(search.toLowerCase()) || (t.result||'').toLowerCase().includes(search.toLowerCase());
    const matchStatus = !statusFilter || t.status === statusFilter;
    return matchSearch && matchStatus;
  });
  const agentTasks = (agentId: string) => filteredTasks.filter(t => t.agent_id === agentId).slice(-10);

  return (
    <div className="flex-1 overflow-y-auto p-6">
      <div className="flex items-center gap-3 mb-4">
        <div className="flex-1 relative">
          <svg className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-600" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z"/></svg>
          <input type="text" placeholder="Search tasks..." value={search} onChange={e => setSearch(e.target.value)}
            className="w-full pl-9 pr-4 py-2 rounded-lg bg-[#0d0d15] border border-gray-800 text-sm text-gray-300 placeholder-gray-600 focus:outline-none focus:border-indigo-500 transition-colors" />
        </div>
        <select value={statusFilter} onChange={e => setStatusFilter(e.target.value)}
          className="px-3 py-2 rounded-lg bg-[#0d0d15] border border-gray-800 text-sm text-gray-400 focus:outline-none focus:border-indigo-500">
          <option value="">All Status</option>
          <option value="completed">Completed</option>
          <option value="running">Running</option>
          <option value="pending">Pending</option>
          <option value="failed">Failed</option>
        </select>
      </div>
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4 mb-6">
        {agents.map(a => (
          <AgentCard key={a.id} agent={a} tasks={agentTasks(a.id)} orgId={orgId} onUpdate={load} />
        ))}
        {tasks.filter(t=>t.agent_id===agent.id).slice(-10).length === 0 && !agentProgress[agent.id] && (
          <div className="col-span-full text-center py-20 text-gray-600">
            <div className="text-4xl mb-3">🤖</div>
            <p className="text-sm">No agents yet. Create your first agent to get started.</p>
          </div>
        )}
      </div>

      {/* Event Log */}
      <div className="rounded-xl border border-gray-800 bg-[#0d0d15] overflow-hidden">
        <div className="px-4 py-2.5 border-b border-gray-800 flex items-center gap-2">
          <div className="w-2 h-2 rounded-full bg-green-500 animate-pulse" />
          <span className="text-xs font-medium text-gray-400">LIVE EVENTS</span>
          <span className="text-xs text-gray-600 ml-auto">{events.length} events</span>
        </div>
        <div className="p-3 max-h-48 overflow-y-auto font-mono text-xs text-gray-500 space-y-1">
          {events.slice(-30).map((e, i) => (
            <div key={i} className="hover:text-gray-300 transition-colors">{e}</div>
          ))}
          {events.length === 0 && <div className="text-gray-700">Waiting for events...</div>}
        </div>
      </div>
    </div>
  );
}

// ── Agent Card ──
function AgentCard({ agent, tasks, orgId, onUpdate }: { agent: Agent; tasks: Task[]; orgId: string; onUpdate: () => void }) {
  const [showTask, setShowTask] = useState(false);
  const pulse = agent.status === 'working';

  const statusColor: Record<string, string> = {
    idle: 'bg-green-500', working: 'bg-amber-500', completed: 'bg-blue-500', error: 'bg-red-500'
  };
  const taskColor: Record<string, string> = {
    pending: 'text-gray-500', running: 'text-amber-400', completed: 'text-green-400', failed: 'text-red-400'
  };

  return (
    <div className={`rounded-xl border bg-[#0d0d15] overflow-hidden transition-all duration-300 ${
      pulse ? 'border-amber-500/30 shadow-[0_0_20px_rgba(245,158,11,0.1)]' : 'border-gray-800 hover:border-gray-700'
    }`}>
      {/* Header */}
      <div className="p-4 border-b border-gray-800">
        <div className="flex items-start justify-between mb-2">
          <div>
            <h3 className="font-semibold text-sm">{agent.name}</h3>
            <p className="text-xs text-gray-500 mt-0.5">{agent.role}</p>
          </div>
          <span className={`inline-flex items-center gap-1.5 px-2 py-0.5 rounded-full text-xs font-medium ${
            agent.status === 'idle' ? 'bg-green-500/10 text-green-400' :
            agent.status === 'working' ? 'bg-amber-500/10 text-amber-400' :
            agent.status === 'error' ? 'bg-red-500/10 text-red-400' :
            'bg-blue-500/10 text-blue-400'
          }`}>
            <span className={`w-1.5 h-1.5 rounded-full ${statusColor[agent.status]} ${pulse ? 'animate-pulse' : ''}`} />
            {agent.status}
          </span>
        </div>
        <p className="text-xs text-gray-600 line-clamp-2">{agent.description}</p>
              <div className="flex gap-4 mt-3 pt-2 border-t border-gray-800">
                <div className="text-center">
                  <div className="text-xs font-semibold text-gray-300">{agent.total_tasks}</div>
                  <div className="text-[10px] text-gray-600">Total</div>
                </div>
                <div className="text-center">
                  <div className="text-xs font-semibold text-green-400">{agent.completed_tasks}</div>
                  <div className="text-[10px] text-gray-600">Done</div>
                </div>
                <div className="text-center">
                  <div className="text-xs font-semibold text-indigo-400">{agent.total_tasks > 0 ? Math.round(agent.completed_tasks/agent.total_tasks*100) : 0}%</div>
                  <div className="text-[10px] text-gray-600">Rate</div>
                </div>
              </div>
      </div>

      {/* Tasks */}
      <div className="px-4 py-3 space-y-2 min-h-[80px]">
        {tasks.map(t => (
          <div key={t.id} className="flex items-start gap-2 text-xs">
            <span className={taskColor[t.status]}>{t.status === 'completed' ? '✅' : t.status === 'running' ? '🔄' : t.status === 'failed' ? '❌' : '⏳'}</span>
            <div className="flex-1 min-w-0">
              <span className="truncate block">{t.description}</span>
              {t.result && <span className="text-green-500/70">{t.result.slice(0, 60)}</span>}
            </div>
          </div>
        ))}
        {tasks.filter(t=>t.agent_id===agent.id).slice(-10).length === 0 && !agentProgress[agent.id] && (
          <p className="text-xs text-gray-700 text-center py-3">No tasks assigned</p>
        )}
      </div>

      {/* Action */}
      <div className="p-3 border-t border-gray-800">
        <button onClick={() => setShowTask(true)} disabled={agent.status === 'working'}
          className={`w-full py-2 rounded-lg text-xs font-medium transition-all duration-200 ${
            agent.status === 'working'
              ? 'bg-gray-800 text-gray-600 cursor-not-allowed'
              : 'bg-indigo-600/10 text-indigo-400 hover:bg-indigo-600/20 border border-indigo-500/20'
          }`}>
          {agent.status === 'working' ? '⏳ Processing...' : '📝 Assign New Task'}
        </button>
      </div>

      <AssignTaskModal agent={agent} orgId={orgId} open={showTask} onClose={() => { setShowTask(false); onUpdate(); }} />
    </div>
  );
}

// ── Modals ──

function Modal({ open, onClose, title, children }: { open: boolean; onClose: () => void; title: string; children: React.ReactNode }) {
  if (!open) return null;
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/60 backdrop-blur-sm" onClick={onClose} />
      <div className="relative bg-[#0d0d15] border border-gray-800 rounded-2xl w-full max-w-md mx-4 shadow-2xl">
        <div className="flex items-center justify-between p-5 border-b border-gray-800">
          <h2 className="text-base font-semibold">{title}</h2>
          <button onClick={onClose} className="w-7 h-7 rounded-lg hover:bg-gray-800 flex items-center justify-center text-gray-500 hover:text-gray-300 transition-colors">✕</button>
        </div>
        <div className="p-5">{children}</div>
      </div>
    </div>
  );
}

function CreateOrgModal({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [name, setName] = useState('');
  const [desc, setDesc] = useState('');
  const [loading, setLoading] = useState(false);
  const setSelected = useOrgStore(s => s.setSelectedOrg);

  const submit = async () => {
    if (!name.trim()) return;
    setLoading(true);
    const r = await fetch(`${API}/api/orgs`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ name, description: desc }) });
    const org = await r.json();
    setSelected(org.id);
    setLoading(false);
    onClose();
  };

  return (
    <Modal open={open} onClose={onClose} title="Create Organization">
      <div className="space-y-4">
        <div>
          <label className="block text-xs font-medium text-gray-400 mb-1.5">Name</label>
          <input value={name} onChange={e => setName(e.target.value)} placeholder="Engineering Team" autoFocus
            className="w-full px-3 py-2 rounded-lg bg-gray-900 border border-gray-700 text-sm text-gray-200 placeholder-gray-600 focus:outline-none focus:border-indigo-500 transition-colors" />
        </div>
        <div>
          <label className="block text-xs font-medium text-gray-400 mb-1.5">Description</label>
          <textarea value={desc} onChange={e => setDesc(e.target.value)} placeholder="What does this team do?" rows={2}
            className="w-full px-3 py-2 rounded-lg bg-gray-900 border border-gray-700 text-sm text-gray-200 placeholder-gray-600 focus:outline-none focus:border-indigo-500 transition-colors resize-none" />
        </div>
        <div className="flex gap-3 pt-2">
          <button onClick={onClose} className="flex-1 py-2 rounded-lg bg-gray-800 text-gray-400 text-sm font-medium hover:bg-gray-700 transition-colors">Cancel</button>
          <button onClick={submit} disabled={loading || !name.trim()}
            className="flex-1 py-2 rounded-lg bg-indigo-600 text-white text-sm font-medium hover:bg-indigo-500 disabled:opacity-50 transition-colors">
            {loading ? 'Creating...' : 'Create'}
          </button>
        </div>
      </div>
    </Modal>
  );
}

function CreateAgentModal({ orgId, open, onClose }: { orgId: string; open: boolean; onClose: () => void }) {
  const [name, setName] = useState('');
  const [role, setRole] = useState('');
  const [desc, setDesc] = useState('');
  const [loading, setLoading] = useState(false);

  const ROLE_TEMPLATES = [
    { role: 'Code Reviewer', desc: 'Reviews code for safety, performance, and idiomatic patterns. Flags unsafe blocks, unwrap() usage, and suggests better alternatives.' },
    { role: 'Security Auditor', desc: 'Audits codebase for security vulnerabilities. Runs cargo-audit, scans for hardcoded secrets, and checks input validation.' },
    { role: 'DevOps Engineer', desc: 'Manages CI/CD pipelines, Docker configurations, and deployment automation. Optimizes build times and infrastructure.' },
    { role: 'Tech Writer', desc: 'Generates and maintains documentation. Writes READMEs, API docs, and architecture guides from code analysis.' },
    { role: 'QA Engineer', desc: 'Writes and runs test suites. Ensures test coverage, identifies edge cases, and creates integration tests.' },
  ];

  const applyTemplate = (t: typeof ROLE_TEMPLATES[number]) => {
    setRole(t.role); setDesc(t.desc);
  };

  const submit = async () => {
    if (!name.trim() || !role.trim()) return;
    setLoading(true);
    await fetch(`${API}/api/orgs/${orgId}/agents`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ name, role, description: desc }) });
    setLoading(false);
    onClose();
  };

  return (
    <Modal open={open} onClose={onClose} title="Create Agent">
      <div className="space-y-4">
        <div>
          <label className="block text-xs font-medium text-gray-400 mb-1.5">Agent Name</label>
          <input value={name} onChange={e => setName(e.target.value)} placeholder="e.g. SecurityBot" autoFocus
            className="w-full px-3 py-2 rounded-lg bg-gray-900 border border-gray-700 text-sm text-gray-200 placeholder-gray-600 focus:outline-none focus:border-indigo-500 transition-colors" />
        </div>
        <div>
          <label className="block text-xs font-medium text-gray-400 mb-1.5">Role</label>
          <input value={role} onChange={e => setRole(e.target.value)} placeholder="e.g. Code Reviewer"
            className="w-full px-3 py-2 rounded-lg bg-gray-900 border border-gray-700 text-sm text-gray-200 placeholder-gray-600 focus:outline-none focus:border-indigo-500 transition-colors" />
        </div>
        <div>
          <label className="block text-xs font-medium text-gray-400 mb-1.5">Role Templates</label>
          <div className="flex flex-wrap gap-1.5">
            {ROLE_TEMPLATES.map(t => (
              <button key={t.role} onClick={() => applyTemplate(t)}
                className="px-2.5 py-1 rounded-md text-xs bg-gray-800 border border-gray-700 text-gray-400 hover:border-indigo-500 hover:text-indigo-400 transition-colors">
                {t.role}
              </button>
            ))}
          </div>
        </div>
        <div>
          <label className="block text-xs font-medium text-gray-400 mb-1.5">Role Description</label>
          <textarea value={desc} onChange={e => setDesc(e.target.value)} placeholder="Describe what this agent does in natural language..."
            rows={3} className="w-full px-3 py-2 rounded-lg bg-gray-900 border border-gray-700 text-sm text-gray-200 placeholder-gray-600 focus:outline-none focus:border-indigo-500 transition-colors resize-none" />
        </div>
        <div className="flex gap-3 pt-2">
          <button onClick={onClose} className="flex-1 py-2 rounded-lg bg-gray-800 text-gray-400 text-sm font-medium hover:bg-gray-700 transition-colors">Cancel</button>
          <button onClick={submit} disabled={loading || !name.trim() || !role.trim()}
            className="flex-1 py-2 rounded-lg bg-indigo-600 text-white text-sm font-medium hover:bg-indigo-500 disabled:opacity-50 transition-colors">
            {loading ? 'Creating...' : 'Create Agent'}
          </button>
        </div>
      </div>
    </Modal>
  );
}

function AssignTaskModal({ agent, orgId, open, onClose }: { agent: Agent; orgId: string; open: boolean; onClose: () => void }) {
  const [desc, setDesc] = useState('');
  const [loading, setLoading] = useState(false);

  const submit = async () => {
    if (!desc.trim()) return;
    setLoading(true);
    await fetch(`${API}/api/orgs/${orgId}/agents/${agent.id}/tasks`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ description: desc }) });
    setLoading(false);
    setDesc('');
    onClose();
  };

  return (
    <Modal open={open} onClose={onClose} title={`Assign Task → ${agent.name}`}>
      <div className="space-y-4">
        <div className="text-xs text-gray-500">
          Agent role: <span className="text-indigo-400">{agent.role}</span> — {agent.description}
        </div>
        <div>
          <label className="block text-xs font-medium text-gray-400 mb-1.5">Task Description</label>
          <p className="text-xs text-gray-600 mb-2">Describe the task in natural language. The agent will use its role definition to understand and execute it.</p>
          <textarea value={desc} onChange={e => setDesc(e.target.value)} placeholder="e.g. Review all unwrap() usage in src/ and suggest safer alternatives using ? or match"
            rows={4} autoFocus
            className="w-full px-3 py-2 rounded-lg bg-gray-900 border border-gray-700 text-sm text-gray-200 placeholder-gray-600 focus:outline-none focus:border-indigo-500 transition-colors resize-none" />
        </div>
        <div className="flex gap-3 pt-2">
          <button onClick={onClose} className="flex-1 py-2 rounded-lg bg-gray-800 text-gray-400 text-sm font-medium hover:bg-gray-700 transition-colors">Cancel</button>
          <button onClick={submit} disabled={loading || !desc.trim()}
            className="flex-1 py-2 rounded-lg bg-indigo-600 text-white text-sm font-medium hover:bg-indigo-500 disabled:opacity-50 transition-colors">
            {loading ? 'Assigning...' : 'Assign Task'}
          </button>
        </div>
      </div>
    </Modal>
  );
}

// ── Zustand-style store (lightweight) ──

function createStore<T>(initial: T) {
  let state = initial;
  const listeners = new Set<() => void>();
  return {
    get: () => state,
    set: (fn: (s: T) => T) => { state = fn(state); listeners.forEach(l => l()); },
    sub: (l: () => void) => { listeners.add(l); return () => listeners.delete(l); },
  };
}

const orgStore = createStore<{ selectedOrg: string | null; setSelectedOrg: (id: string | null) => void }>({
  selectedOrg: null,
  setSelectedOrg: () => {},
});

function useOrgStore<T>(selector: (s: { selectedOrg: string | null; setSelectedOrg: (id: string | null) => void }) => T): T {
  const [, force] = useState({});
  useEffect(() => orgStore.sub(() => force({})), []);
  return selector(orgStore.get());
}

// Initialize store
orgStore.set(s => ({ ...s, setSelectedOrg: (id: string | null) => orgStore.set(s => ({ ...s, selectedOrg: id })) }));
