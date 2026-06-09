import React, { useState, useEffect, useCallback } from 'react';

const API = 'http://127.0.0.1:4000';
const WS = 'ws://127.0.0.1:4000/api/ws';

type Org = { id: string; name: string; description: string };
type Agent = { id: string; org_id: string; name: string; role: string; description: string; status: 'idle'|'working'|'completed'|'error'; current_task?: string };
type Task = { id: string; org_id: string; agent_id: string; description: string; status: 'pending'|'running'|'completed'|'failed'; result?: string };

export default function App() {
  const [orgs, setOrgs] = useState<Org[]>([]);
  const [selectedOrg, setSelectedOrg] = useState<string | null>(null);
  const [agents, setAgents] = useState<Agent[]>([]);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [events, setEvents] = useState<string[]>([]);

  const loadOrgs = useCallback(async () => {
    const r = await fetch(`${API}/api/orgs`);
    setOrgs(await r.json());
  }, []);

  const loadAgents = useCallback(async (orgId: string) => {
    const r = await fetch(`${API}/api/orgs/${orgId}/agents`);
    setAgents(await r.json());
  }, []);

  const loadTasks = useCallback(async (orgId: string) => {
    const r = await fetch(`${API}/api/orgs/${orgId}/agents/_/tasks`);
    setTasks(await r.json());
  }, []);

  useEffect(() => { loadOrgs(); }, [loadOrgs]);

  useEffect(() => {
    if (!selectedOrg) return;
    loadAgents(selectedOrg);
    loadTasks(selectedOrg);
  }, [selectedOrg, loadAgents, loadTasks]);

  // WebSocket for real-time updates
  useEffect(() => {
    const ws = new WebSocket(WS);
    ws.onmessage = (e) => {
      setEvents(prev => [...prev.slice(-50), e.data]);
      if (selectedOrg) {
        loadAgents(selectedOrg);
        loadTasks(selectedOrg);
      }
    };
    return () => ws.close();
  }, [selectedOrg, loadAgents, loadTasks]);

  const createOrg = async () => {
    const name = prompt('Organization name:');
    const desc = prompt('Description:');
    if (!name) return;
    await fetch(`${API}/api/orgs`, { method:'POST', headers:{'Content-Type':'application/json'}, body:JSON.stringify({name,description:desc||''}) });
    loadOrgs();
  };

  const createAgent = async () => {
    if (!selectedOrg) return;
    const name = prompt('Agent name:');
    const role = prompt('Role (e.g. "Code Reviewer", "Security Auditor"):');
    const desc = prompt('Role description (natural language):');
    if (!name || !role) return;
    await fetch(`${API}/api/orgs/${selectedOrg}/agents`, { method:'POST', headers:{'Content-Type':'application/json'}, body:JSON.stringify({name,role,description:desc||''}) });
    loadAgents(selectedOrg);
  };

  const assignTask = async (agentId: string) => {
    if (!selectedOrg) return;
    const desc = prompt('Task description (natural language):');
    if (!desc) return;
    await fetch(`${API}/api/orgs/${selectedOrg}/agents/${agentId}/tasks`, { method:'POST', headers:{'Content-Type':'application/json'}, body:JSON.stringify({description:desc}) });
    loadTasks(selectedOrg);
    loadAgents(selectedOrg);
  };

  return (
    <div style={{ display:'flex', height:'100vh', fontFamily:'system-ui' }}>
      {/* Sidebar */}
      <div style={{ width:260, background:'#1a1a2e', color:'#eee', padding:16, overflowY:'auto' }}>
        <h2 style={{ margin:'0 0 16px', fontSize:18 }}>🏢 Organizations</h2>
        <button onClick={createOrg} style={{ width:'100%', padding:8, marginBottom:12, background:'#0f3460', color:'#eee', border:'none', borderRadius:4, cursor:'pointer' }}>
          + New Organization
        </button>
        {orgs.map(o => (
          <div key={o.id} onClick={() => setSelectedOrg(o.id)}
            style={{ padding: '10px 12px', marginBottom:4, borderRadius:6, cursor:'pointer',
              background: selectedOrg===o.id ? '#16213e' : 'transparent', transition:'0.2s' }}>
            <div style={{ fontWeight:600 }}>{o.name}</div>
            <div style={{ fontSize:12, opacity:0.7 }}>{o.description.slice(0,60)}</div>
          </div>
        ))}
      </div>

      {/* Main */}
      <div style={{ flex:1, display:'flex', flexDirection:'column', background:'#f5f5f5' }}>
        {selectedOrg ? (
          <>
            {/* Header */}
            <div style={{ padding:'16px 24px', background:'#fff', borderBottom:'1px solid #ddd', display:'flex', justifyContent:'space-between', alignItems:'center' }}>
              <h1 style={{ margin:0, fontSize:20 }}>{orgs.find(o=>o.id===selectedOrg)?.name || 'Organization'}</h1>
              <button onClick={createAgent} style={{ padding:'8px 16px', background:'#0f3460', color:'#fff', border:'none', borderRadius:4, cursor:'pointer' }}>
                + Create Agent
              </button>
            </div>

            {/* Agent Grid */}
            <div style={{ padding:24, display:'grid', gridTemplateColumns:'repeat(auto-fill, minmax(280px, 1fr))', gap:16, overflowY:'auto' }}>
              {agents.map(a => (
                <div key={a.id} style={{ background:'#fff', borderRadius:8, padding:16, boxShadow:'0 1px 4px rgba(0,0,0,0.08)' }}>
                  <div style={{ display:'flex', justifyContent:'space-between', alignItems:'center', marginBottom:8 }}>
                    <strong>{a.name}</strong>
                    <StatusBadge status={a.status} />
                  </div>
                  <div style={{ color:'#666', fontSize:13, marginBottom:4 }}>🎭 {a.role}</div>
                  <div style={{ color:'#888', fontSize:12, marginBottom:12 }}>{a.description.slice(0,80)}</div>

                  {a.current_task && (
                    <div style={{ background:'#fff3cd', padding:'6px 8px', borderRadius:4, fontSize:12, marginBottom:12 }}>
                      ⚡ Working: {tasks.find(t=>t.id===a.current_task)?.description?.slice(0,50) || '...'}
                    </div>
                  )}

                  <button onClick={() => assignTask(a.id)} disabled={a.status==='working'}
                    style={{ width:'100%', padding:'6px', border:'1px solid #0f3460', borderRadius:4, background:a.status==='working'?'#eee':'#fff', color:a.status==='working'?'#999':'#0f3460', cursor:a.status==='working'?'not-allowed':'pointer', fontSize:12 }}>
                    {a.status==='working' ? '⏳ Working...' : '📝 Assign Task'}
                  </button>

                  {/* Agent tasks */}
                  {tasks.filter(t=>t.agent_id===a.id).slice(0,5).map(t => (
                    <div key={t.id} style={{ marginTop:8, padding:'4px 6px', background:t.status==='completed'?'#d4edda':'#f8f9fa', borderRadius:4, fontSize:11 }}>
                      <TaskStatusIcon status={t.status} /> {t.description.slice(0,60)}
                      {t.result && <div style={{ color:'#28a745', marginTop:2 }}>{t.result}</div>}
                    </div>
                  ))}
                </div>
              ))}

              {agents.length===0 && (
                <div style={{ gridColumn:'1/-1', textAlign:'center', color:'#999', padding:40 }}>
                  No agents yet. Create one to get started.
                </div>
              )}
            </div>

            {/* Event Log */}
            <div style={{ margin: '0 24px 24px', background:'#fff', borderRadius:8, padding:12, fontSize:11, fontFamily:'monospace', maxHeight:200, overflowY:'auto', boxShadow:'0 1px 4px rgba(0,0,0,0.04)' }}>
              <strong style={{ fontSize:13 }}>📡 Live Events</strong>
              {events.slice(-20).map((e,i) => <div key={i} style={{ marginTop:4, color:'#555' }}>{e}</div>)}
              {events.length===0 && <div style={{ color:'#999' }}>Waiting for events...</div>}
            </div>
          </>
        ) : (
          <div style={{ display:'flex', alignItems:'center', justifyContent:'center', flex:1, color:'#999', fontSize:18 }}>
            Select or create an organization to begin
          </div>
        )}
      </div>
    </div>
  );
}

function StatusBadge({ status }: { status: string }) {
  const colors: Record<string,string> = { idle:'#28a745', working:'#ffc107', completed:'#007bff', error:'#dc3545' };
  return <span style={{ fontSize:11, padding:'2px 8px', borderRadius:10, background:colors[status]||'#999', color:'#fff' }}>{status}</span>;
}

function TaskStatusIcon({ status }: { status: string }) {
  const icons: Record<string,string> = { pending:'⏳', running:'🔄', completed:'✅', failed:'❌' };
  return <span>{icons[status]||'?'}</span>;
}
