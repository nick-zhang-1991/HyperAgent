import './index.css';
import AuthPage from './AuthPage';
import { tt, sl, al, gl } from './i18n';
import { apiBase, wsBase } from './api';
import { useState, useEffect, useCallback } from 'react';

const BG = 'linear-gradient(135deg, #05051a 0%, #0a0a2e 30%, #0d0d28 60%, #060622 100%)';
const CARD = 'rgba(15,15,40,0.7)';
const BORDER = 'rgba(99,102,241,0.12)';
const INPUT = 'rgba(10,10,25,0.8)';
const GRAD = 'linear-gradient(135deg, #6366f1, #8b5cf6)';
const TEXT = '#e2e8f0';
const MUTED = 'rgba(148,163,184,0.7)';

type T = Record<string,any>;

export default function App() {
  const [token, setToken] = useState(localStorage.getItem('token')||'');
  const [user, setUser] = useState<T|null>(null);
  const [orgs, setOrgs] = useState<T[]>([]);
  const [org, setOrg] = useState<T|null>(null);
  const [agents, setAgents] = useState<T[]>([]);
  const [tasks, setTasks] = useState<T[]>([]);
  const [events, setEvents] = useState<string[]>([]);
  const [progress, setProgress] = useState<Record<string,string>>({});
  const [showCreateOrg, setShowCreateOrg] = useState(false);
  const [showCreateAgent, setShowCreateAgent] = useState(false);
  const [showAssignTask, setShowAssignTask] = useState<T|null>(null);
  const [showSettings, setShowSettings] = useState(false);

  const headers = { 'Authorization': `Bearer ${token}`, 'Content-Type': 'application/json' };
  const url = (p: string) => apiBase() + p;
  const auth = useCallback((t:string,u:T)=>{setToken(t);setUser(u);localStorage.setItem('token',t);},[]);
  const logout = ()=>{setToken('');setUser(null);localStorage.removeItem('token');};

  // Two separate effects: identity (me+orgs) is one cycle, while agents+tasks
  // re-fire whenever the selected org changes. Splitting avoids a closure bug
  // where setOrg() set the state mid-fetch but the next `if(org)` still saw
  // the old null value, dropping the agents/tasks load until the 5s interval.
  const loadIdentity = useCallback(async ()=>{
    if(!token)return;
    try{
      const [u,o] = await Promise.all([
        fetch(url('/api/auth/me'),{headers}).then(r=>r.ok?r.json():null),
        fetch(url('/api/orgs'),{headers}).then(r=>r.ok?r.json():[])
      ]);
      setUser(u); setOrgs(o);
      // Auto-select the first org if user has none selected.
      // Use functional setter so we read the latest value, not a stale closure.
      setOrg(prev => prev ?? (o.length>0 ? o[0] : null));
    }catch(e){}
  },[token]);

  const loadAgentsTasks = useCallback(async ()=>{
    if(!token || !org) return;
    try{
      const [a,t] = await Promise.all([
        fetch(url(`/api/orgs/${org.id}/agents`),{headers}).then(r=>r.ok?r.json():[]),
        fetch(url(`/api/orgs/${org.id}/agents/_/tasks`),{headers}).then(r=>r.ok?r.json():[])
      ]);
      setAgents(a); setTasks(t);
    }catch(e){}
  },[token, org]);

  useEffect(()=>{loadIdentity();const i=setInterval(loadIdentity,5000);return ()=>clearInterval(i);},[loadIdentity]);
  useEffect(()=>{loadAgentsTasks();const i=setInterval(loadAgentsTasks,5000);return ()=>clearInterval(i);},[loadAgentsTasks]);

  useEffect(()=>{
    if(!token)return;
    let ws: WebSocket | null = null;
    let retry = 0;
    let cancelled = false;
    let retryTimer: number | null = null;
    const connect = () => {
      if (cancelled) return;
      ws = new WebSocket(wsBase() + '/api/ws');
      ws.onmessage = e=>{
        const data = e.data;
        setEvents(p=>[...p.slice(-50),data]);
        if(data.startsWith('progress:')){
          const[,aid,...r]=data.split(':');
          setProgress(p=>({...p,[aid]:r.join(':')}));
        } else if(data.startsWith('done:') || data.startsWith('fail:') || data.startsWith('task:')) {
          // Task finished — clear any in-flight progress for ALL agents so badges
          // drop back to the agent's real DB status (idle).
          setProgress({});
        }
        loadAgentsTasks();
      };
      ws.onclose = () => {
        if (cancelled) return;
        retry = Math.min(retry + 1, 6);
        const delay = Math.min(1000 * (2 ** (retry - 1)), 15000);
        retryTimer = window.setTimeout(connect, delay);
      };
      ws.onopen = () => { retry = 0; };
    };
    connect();
    return () => { cancelled = true; if (retryTimer) clearTimeout(retryTimer); if (ws) ws.close(); };
  },[token, loadAgentsTasks]);

  if(!token) return <AuthPage onAuth={auth}/>;

  return (
    <div className="min-h-screen flex" style={{background:BG,color:TEXT}}>
      {/* Sidebar */}
      <div className="w-64 flex flex-col p-4 border-r backdrop-blur-sm" style={{background:'rgba(10,10,30,0.9)',borderColor:'rgba(99,102,241,0.1)'}}>
        <div className="flex items-center gap-3 mb-6">
          <div className="w-9 h-9 rounded-xl flex items-center justify-center" style={{background:GRAD}}>
            <svg className="w-5 h-5 text-white" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M9.75 3.104v5.714a2.25 2.25 0 01-.659 1.591L5 14.5M9.75 3.104c-.251.023-.501.05-.75.082m.75-.082a24.301 24.301 0 014.5 0m0 0v5.714c0 .597.237 1.17.659 1.591L19.8 15.3M14.25 3.104c.251.023.501.05.75.082M19.8 15.3l-1.57.393A9.065 9.065 0 0112 15a9.065 9.065 0 00-6.23.693L5 14.5m14.8.8l1.402 1.402c1.232 1.232.65 3.318-1.067 3.611A48.309 48.309 0 0112 21c-2.773 0-5.491-.235-8.135-.687-1.718-.293-2.3-2.379-1.067-3.61L5 14.5" /></svg>
          </div>
          <div><div className="text-sm font-bold" style={{background:GRAD,WebkitBackgroundClip:'text',WebkitTextFillColor:'transparent'}}>HyperAgent</div><div className="text-[10px]" style={{color:MUTED}}>{user?.email}</div></div>
        </div>

        <div className="text-[10px] uppercase tracking-wider mb-2" style={{color:MUTED}}>{tt('orgs')}</div>
        {orgs.map((o:T)=><button key={o.id} onClick={()=>{setOrg(o);setAgents([]);setTasks([]);}}
          className="w-full text-left px-3 py-2 rounded-lg text-xs mb-1 transition-all duration-200"
          style={org?.id===o.id?{background:'rgba(99,102,241,0.15)',color:'#a5b4fc',border:'1px solid rgba(99,102,241,0.25)'}:{color:MUTED}}>
          <div className="font-medium truncate">{o.name}</div><div className="text-[10px] truncate" style={{color:'rgba(148,163,184,0.4)'}}>{o.description}</div>
        </button>)}
        <button onClick={()=>setShowCreateOrg(true)} className="w-full mt-2 py-2 border border-dashed rounded-lg text-xs transition-colors" style={{borderColor:'rgba(99,102,241,0.2)',color:MUTED}}>{tt('newOrg')}</button>
        
        <div className="flex-1" />
        <div className="flex items-center justify-between mb-2">
          <button onClick={()=>setShowSettings(true)} className="text-xs transition-colors" style={{color:MUTED}}>{tt('settings')}</button>
          <select value={gl()} onChange={e=>sl(e.target.value)} className="text-[10px] py-1 px-2 rounded border" style={{background:INPUT,borderColor:'rgba(99,102,241,0.15)',color:MUTED}}>
            {al().map(l=><option key={l.code} value={l.code}>{l.name}</option>)}
          </select>
        </div>
        <button onClick={logout} className="text-xs transition-colors" style={{color:MUTED}}>{tt('logout')}</button>
      </div>

      {/* Main */}
      <div className="flex-1 p-6 overflow-auto">
        {!org ? (
          <div className="flex items-center justify-center h-full text-sm" style={{color:MUTED}}>{tt('selectOrg')}</div>
        ) : (
          <>
            <div className="flex items-center justify-between mb-6">
              <div><h1 className="text-xl font-bold">{org.name}</h1><p className="text-xs" style={{color:MUTED}}>{org.description}</p></div>
              <button onClick={()=>setShowCreateAgent(true)} className="px-5 py-2.5 rounded-xl text-sm font-medium transition-all duration-300 hover:shadow-lg active:scale-[0.98]" style={{background:GRAD,color:'white'}}>{tt('newAgent')}</button>
            </div>

            <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4 mb-6">
              {agents.map((a:T)=><div key={a.id} className="rounded-xl p-5 backdrop-blur-sm border transition-all duration-300 hover:border-opacity-40" style={{background:CARD,borderColor:BORDER}}>
                <div className="flex items-center justify-between mb-2">
                  <h3 className="font-semibold text-sm">{a.name}</h3>
                  <span className="px-2 py-0.5 rounded-full text-[10px] font-medium" style={
                    progress[a.id]?{background:'rgba(250,204,21,0.15)',color:'#facc15'}:
                    a.status==='error'?{background:'rgba(248,113,113,0.15)',color:'#f87171'}:
                    a.status==='completed'?{background:'rgba(34,197,94,0.15)',color:'#22c55e'}:
                    {background:'rgba(99,102,241,0.1)',color:'#a5b4fc'}}>
                    {progress[a.id]?'working':a.status||'idle'}
                  </span>
                </div>
                <p className="text-xs mb-1" style={{color:'#a5b4fc'}}>{a.role}</p>
                <p className="text-xs mb-3 line-clamp-2" style={{color:MUTED}}>{a.description}</p>
                <div className="flex gap-4 pt-3 border-t" style={{borderColor:'rgba(99,102,241,0.08)'}}>
                  <div className="text-center"><div className="text-sm font-bold">{a.total_tasks||0}</div><div className="text-[10px]" style={{color:MUTED}}>Total</div></div>
                  <div className="text-center"><div className="text-sm font-bold" style={{color:'#22c55e'}}>{a.completed_tasks||0}</div><div className="text-[10px]" style={{color:MUTED}}>Done</div></div>
                  <div className="text-center"><div className="text-sm font-bold" style={{color:'#a5b4fc'}}>{a.total_tasks>0?Math.round(a.completed_tasks/a.total_tasks*100):0}%</div><div className="text-[10px]" style={{color:MUTED}}>Rate</div></div>
                </div>
                {progress[a.id] && <div className="mt-3 flex items-center gap-2 text-[10px]" style={{color:'#facc15'}}>
                  <svg className="w-3 h-3 animate-spin" viewBox="0 0 24 24"><circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" fill="none"/><path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"/></svg>
                  {progress[a.id]}
                </div>}
                <div className="mt-3 space-y-1">
                  {tasks.filter((t:T)=>t.agent_id===a.id).slice(-3).map((t:T)=><div key={t.id} className="flex items-start gap-1 text-[10px]">
                    <span>{t.status==='completed'?'✅':t.status==='running'?'🔄':t.status==='failed'?'❌':'⏳'}</span>
                    <span className="truncate" style={{color:MUTED}}>{t.description}</span>
                    {t.result && <span className="block text-[9px]" style={{color:'rgba(34,197,94,0.7)'}}>{t.result.slice(0,50)}</span>}
                  </div>)}
                </div>
                <button onClick={()=>setShowAssignTask(a)} className="w-full mt-3 py-2 rounded-lg text-xs transition-all duration-200" style={{background:INPUT,border:'1px solid rgba(99,102,241,0.1)',color:MUTED}}>{tt('assignTask')}</button>
              </div>)}
              {agents.length===0 && <div className="col-span-full text-center py-20" style={{color:MUTED}}><div className="text-4xl mb-3">🤖</div><p className="text-sm">{tt('noAgents')}</p></div>}
            </div>

            <div className="rounded-xl p-4 border backing-blur-sm" style={{background:CARD,borderColor:BORDER}}>
              <div className="text-xs font-semibold mb-2" style={{color:MUTED}}>{tt('liveEvents')} ({events.length})</div>
              <div className="max-h-32 overflow-y-auto space-y-1">
                {events.slice(-15).map((e,i)=><div key={i} className="text-[10px] font-mono" style={{color:'rgba(148,163,184,0.4)'}}>{e}</div>)}
                {events.length===0 && <div className="text-[10px]" style={{color:'rgba(148,163,184,0.25)'}}>{tt('waiting')}</div>}
              </div>
            </div>
          </>
        )}
      </div>

      {/* Modals */}
      {showCreateOrg && <Modal onClose={()=>setShowCreateOrg(false)} title={tt('newOrg')}>
        <form onSubmit={async e=>{e.preventDefault();const f=new FormData(e.currentTarget);const r=await fetch(url('/api/orgs'),{method:'POST',headers,body:JSON.stringify({name:f.get('name'),description:f.get('desc')})});if(r.ok){const newOrg=await r.json();setOrg(newOrg);setOrgs(p=>[newOrg,...p]);}setShowCreateOrg(false);loadIdentity();}}>
          <Field name="name" label={tt('name')} />
          <Field name="desc" label={tt('description')} />
          <Submit label={tt('create')} />
        </form>
      </Modal>}
      {showCreateAgent && <Modal onClose={()=>setShowCreateAgent(false)} title={tt('newAgent')}>
        <form onSubmit={async e=>{e.preventDefault();const f=new FormData(e.currentTarget);const r=await fetch(url(`/api/orgs/${org!.id}/agents`),{method:'POST',headers,body:JSON.stringify({name:f.get('name'),role:f.get('role'),description:f.get('desc')})});if(r.ok){const newA=await r.json();setAgents(p=>[newA,...p]);}setShowCreateAgent(false);loadAgentsTasks();}}>
          <Field name="name" label={tt('name')} />
          <div className="mb-3"><label className="block text-[10px] mb-1" style={{color:MUTED}}>{tt('role')}</label><select name="role" className="w-full px-4 py-3 rounded-xl text-sm outline-none" style={{background:INPUT,border:'1px solid rgba(99,102,241,0.15)',color:TEXT}} onChange={e=>{const d=(document.querySelector('[name=desc]')as HTMLInputElement);const roles:Record<string,string>={'Developer':'Code, architecture','Reviewer':'Safety, quality','Tester':'Test, verify','DevOps':'CI/CD, deploy','Analyst':'Data, reports'};if(d)d.value=roles[e.target.value]||'';}}>
              <option value="">Select...</option>
              {['Developer','Reviewer','Tester','DevOps','Analyst'].map(r=><option key={r}>{r}</option>)}
            </select></div>
          <Field name="desc" label={tt('description')} />
          <Submit label={tt('create')} />
        </form>
      </Modal>}
      {showAssignTask && <Modal onClose={()=>setShowAssignTask(null)} title={`${tt('assignTask')} → ${showAssignTask.name}`}>
        <p className="text-xs mb-4" style={{color:MUTED}}>{showAssignTask.role}</p>
        <form onSubmit={async e=>{e.preventDefault();const f=new FormData(e.currentTarget);const r=await fetch(url(`/api/orgs/${org!.id}/agents/${showAssignTask.id}/tasks`),{method:'POST',headers,body:JSON.stringify({description:f.get('task')})});if(r.ok){const newT=await r.json();setTasks(p=>[newT,...p]);}setShowAssignTask(null);loadAgentsTasks();}}>
          <textarea name="task" placeholder={tt('taskDesc')} className="w-full px-4 py-3 rounded-xl text-sm mb-4 h-24 resize-none outline-none" style={{background:INPUT,border:'1px solid rgba(99,102,241,0.15)',color:TEXT}} autoFocus />
          <Submit label={tt('assignTask')} />
        </form>
      </Modal>}
      {showSettings && <Modal onClose={()=>setShowSettings(false)} title={tt('settings')}>
        <form onSubmit={async e=>{e.preventDefault();const f=new FormData(e.currentTarget);await fetch(url('/api/auth/model'),{method:'POST',headers,body:JSON.stringify({provider:f.get('provider'),model:f.get('model'),api_key:f.get('key'),temperature:parseFloat(f.get('temp')as string)})});setShowSettings(false);}}>
          <div className="mb-3"><label className="block text-[10px] mb-1" style={{color:MUTED}}>{tt('provider')}</label><select name="provider" defaultValue={user?.model?.provider||'openai'} className="w-full px-4 py-3 rounded-xl text-sm outline-none" style={{background:INPUT,border:'1px solid rgba(99,102,241,0.15)',color:TEXT}}>{['openai','anthropic','deepseek','groq','together','local'].map(p=><option key={p}>{p}</option>)}</select></div>
          <Field name="model" label={tt('model')} defaultValue={user?.model?.model||'gpt-4o'} />
          <Field name="key" label={tt('apiKey')} type="password" defaultValue={user?.model?.api_key||''} />
          <div className="mb-4"><label className="block text-[10px] mb-1" style={{color:MUTED}}>{tt('temperature')}: {(user?.model?.temperature||0.7).toFixed(1)}</label><input name="temp" type="range" min="0" max="2" step="0.1" defaultValue={user?.model?.temperature||0.7} className="w-full accent-indigo-500" /></div>
          <Submit label={tt('save')} />
        </form>
      </Modal>}
    </div>
  );
}

function Field({name,label,type='text',defaultValue=''}:{name:string;label:string;type?:string;defaultValue?:string}) {
  return <div className="mb-3"><label className="block text-[10px] mb-1" style={{color:MUTED}}>{label}</label><input name={name} type={type} defaultValue={defaultValue} className="w-full px-4 py-3 rounded-xl text-sm outline-none" style={{background:INPUT,border:'1px solid rgba(99,102,241,0.15)',color:TEXT}} /></div>;
}

function Submit({label}:{label:string}) {
  return <button type="submit" className="w-full py-3 rounded-xl text-sm font-medium transition-all duration-300 hover:shadow-lg active:scale-[0.98]" style={{background:GRAD,color:'white'}}>{label}</button>;
}

function Modal({children,onClose,title}:{children:React.ReactNode;onClose:()=>void;title:string}) {
  return <div className="fixed inset-0 flex items-center justify-center z-50 p-4" style={{background:'rgba(0,0,0,0.6)',backdropFilter:'blur(4px)'}} onClick={onClose}>
    <div className="rounded-2xl p-6 w-full max-w-sm border backing-blur-xl" style={{background:'rgba(15,15,45,0.95)',borderColor:'rgba(99,102,241,0.2)'}} onClick={e=>e.stopPropagation()}>
      <h2 className="text-lg font-bold mb-4">{title}</h2>
      {children}
    </div>
  </div>;
}
