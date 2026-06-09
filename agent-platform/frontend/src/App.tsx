import './index.css';
import AuthPage from './AuthPage';
import { useState, useEffect, useCallback } from 'react';

const API = 'http://127.0.0.1:4000';
const WS = 'ws://127.0.0.1:4000/api/ws';

type T = Record<string,any>;

export default function App() {
  const [token, setToken] = useState(localStorage.getItem('token') || '');
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

  const auth = useCallback((t:string,u:T)=>{setToken(t);setUser(u);localStorage.setItem('token',t);},[]);
  const logout = ()=>{setToken('');setUser(null);localStorage.removeItem('token');};

  const load = useCallback(async ()=>{
    if(!token)return;
    try{
      const [u,o] = await Promise.all([
        fetch(API+'/api/auth/me',{headers}).then(r=>r.ok?r.json():null),
        fetch(API+'/api/orgs',{headers}).then(r=>r.ok?r.json():[])
      ]);
      setUser(u); setOrgs(o);
      if(o.length>0 && !org) setOrg(o[0]);
      if(org){
        const [a,t] = await Promise.all([
          fetch(`${API}/api/orgs/${org!.id}/agents`,{headers}).then(r=>r.ok?r.json():[]),
          fetch(`${API}/api/orgs/${org!.id}/agents/_/tasks`,{headers}).then(r=>r.ok?r.json():[])
        ]);
        setAgents(a); setTasks(t);
      }
    }catch(e){console.warn('Backend not available')}
  },[token,org]);

  useEffect(()=>{load();const i=setInterval(load,3000);return ()=>clearInterval(i);},[load]);
  useEffect(()=>{
    if(!token)return;
    const ws = new WebSocket(WS);
    ws.onmessage = e=>{
      setEvents(p=>[...p.slice(-50),e.data]);
      if(e.data.startsWith('progress:')) {
        const [,aid,...rest]=e.data.split(':');
        setProgress(p=>({...p,[aid]:rest.join(':')}));
      }
      load();
    };
    return ()=>ws.close();
  },[token]);

  if(!token) return <AuthPage onAuth={auth}/>;

  return (
    <div className="min-h-screen bg-[#0a0a0f] text-white flex">
      {/* Sidebar */}
      <div className="w-64 bg-[#0d0d15] border-r border-gray-800 p-4 flex flex-col">
        <div className="flex items-center gap-3 mb-6">
          <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-indigo-500 to-purple-600 flex items-center justify-center font-bold text-sm">A</div>
          <div><div className="text-sm font-semibold">Agent Platform</div><div className="text-[10px] text-gray-500">{user?.email}</div></div>
        </div>
        <div className="text-[10px] text-gray-600 uppercase tracking-wider mb-2">Organizations</div>
        {orgs.map((o:T)=><button key={o.id} onClick={()=>{setOrg(o);setAgents([]);setTasks([]);}}
          className={`w-full text-left px-3 py-2 rounded-lg text-xs mb-1 transition-colors ${org?.id===o.id?'bg-indigo-600/20 text-indigo-400 border border-indigo-600/30':'text-gray-400 hover:bg-[#06060a]'}`}>
          <div className="font-medium truncate">{o.name}</div><div className="text-[10px] text-gray-600 truncate">{o.description}</div>
        </button>)}
        <button onClick={()=>setShowCreateOrg(true)} className="w-full mt-2 py-1.5 border border-dashed border-gray-700 rounded-lg text-xs text-gray-500 hover:text-indigo-400 hover:border-indigo-600/30">+ New Org</button>
        <div className="flex-1"/>
        <button onClick={()=>setShowSettings(true)} className="text-xs text-gray-500 hover:text-white mb-2">⚙ Settings</button>
        <button onClick={logout} className="text-xs text-gray-500 hover:text-red-400">Sign Out</button>
      </div>

      {/* Main */}
      <div className="flex-1 p-6 overflow-auto">
        {!org ? (
          <div className="flex items-center justify-center h-full text-gray-600 text-sm">Select or create an organization</div>
        ) : (
          <>
            <div className="flex items-center justify-between mb-6">
              <div><h1 className="text-xl font-bold">{org.name}</h1><p className="text-xs text-gray-500">{org.description}</p></div>
              <button onClick={()=>setShowCreateAgent(true)} className="px-4 py-2 bg-indigo-600 hover:bg-indigo-500 rounded-lg text-sm">+ Agent</button>
            </div>

            {/* Agent Cards */}
            <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4 mb-6">
              {agents.map((a:T)=><div key={a.id} className="bg-[#0d0d15] border border-gray-800 rounded-xl p-4 hover:border-indigo-600/30 transition-colors">
                <div className="flex items-center justify-between mb-2">
                  <h3 className="font-semibold text-sm">{a.name}</h3>
                  <span className={`px-2 py-0.5 rounded-full text-[10px] ${
                    a.status==='working'?'bg-yellow-500/20 text-yellow-400':
                    a.status==='completed'?'bg-green-500/20 text-green-400':
                    a.status==='error'?'bg-red-500/20 text-red-400':'bg-gray-500/20 text-gray-400'}`}>
                    {progress[a.id] ? 'working' : a.status||'idle'}
                  </span>
                </div>
                <p className="text-xs text-gray-500 mb-1">{a.role}</p>
                <p className="text-xs text-gray-600 line-clamp-2 mb-3">{a.description}</p>
                <div className="flex gap-4 pt-2 border-t border-gray-800">
                  <div className="text-center"><div className="text-xs font-semibold">{a.total_tasks||0}</div><div className="text-[10px] text-gray-600">Total</div></div>
                  <div className="text-center"><div className="text-xs font-semibold text-green-400">{a.completed_tasks||0}</div><div className="text-[10px] text-gray-600">Done</div></div>
                  <div className="text-center"><div className="text-xs font-semibold text-indigo-400">{a.total_tasks>0?Math.round(a.completed_tasks/a.total_tasks*100):0}%</div><div className="text-[10px] text-gray-600">Rate</div></div>
                </div>
                {progress[a.id] && <div className="mt-2 flex items-center gap-1 text-[10px] text-indigo-400"><svg className="w-3 h-3 animate-spin" viewBox="0 0 24 24"><circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4"/><path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"/></svg>{progress[a.id]}</div>}
                <div className="mt-3 space-y-1">
                  {tasks.filter((t:T)=>t.agent_id===a.id).slice(-3).map((t:T)=><div key={t.id} className="flex items-start gap-1 text-[10px]">
                    <span>{t.status==='completed'?'✅':t.status==='running'?'🔄':t.status==='failed'?'❌':'⏳'}</span>
                    <span className="truncate">{t.description}</span>
                  </div>)}
                </div>
                <button onClick={()=>setShowAssignTask(a)} className="w-full mt-3 py-1.5 bg-[#06060a] border border-gray-800 rounded-lg text-xs text-gray-400 hover:text-white hover:border-indigo-600/30">📝 Assign Task</button>
              </div>)}
              {agents.length===0 && <div className="col-span-full text-center py-20 text-gray-600">
                <div className="text-4xl mb-3">🤖</div><p className="text-sm">No agents yet</p>
              </div>}
            </div>

            {/* Events */}
            <div className="bg-[#0d0d15] border border-gray-800 rounded-xl p-4">
              <div className="text-xs font-semibold text-gray-400 mb-2">📡 Live Events ({events.length})</div>
              <div className="max-h-40 overflow-y-auto space-y-1">
                {events.slice(-20).map((e,i)=><div key={i} className="text-[10px] text-gray-600 font-mono">{e}</div>)}
                {events.length===0 && <div className="text-[10px] text-gray-700">Waiting for events...</div>}
              </div>
            </div>
          </>
        )}
      </div>

      {/* Modals */}
      {showCreateOrg && <Modal onClose={()=>setShowCreateOrg(false)}>
        <h2 className="font-semibold mb-4">New Organization</h2>
        <form onSubmit={async e=>{e.preventDefault();const f=new FormData(e.currentTarget);const r=await fetch(API+'/api/orgs',{method:'POST',headers,body:JSON.stringify({name:f.get('name'),description:f.get('desc')})});if(r.ok){setShowCreateOrg(false);load();}}}>
          <input name="name" placeholder="Name" className="w-full px-3 py-2 rounded-lg bg-[#06060a] border border-gray-800 text-sm mb-3" autoFocus/>
          <input name="desc" placeholder="Description" className="w-full px-3 py-2 rounded-lg bg-[#06060a] border border-gray-800 text-sm mb-4"/>
          <button type="submit" className="w-full py-2 bg-indigo-600 rounded-lg text-sm">Create</button>
        </form>
      </Modal>}
      {showCreateAgent && <Modal onClose={()=>setShowCreateAgent(false)}>
        <h2 className="font-semibold mb-4">New Agent</h2>
        <form onSubmit={async e=>{e.preventDefault();const f=new FormData(e.currentTarget);const r=await fetch(`${API}/api/orgs/${org!.id}/agents`,{method:'POST',headers,body:JSON.stringify({name:f.get('name'),role:f.get('role'),description:f.get('desc')})});if(r.ok){setShowCreateAgent(false);load();}}}>
          <input name="name" placeholder="Name" className="w-full px-3 py-2 rounded-lg bg-[#06060a] border border-gray-800 text-sm mb-3" autoFocus/>
          <select name="role" className="w-full px-3 py-2 rounded-lg bg-[#06060a] border border-gray-800 text-sm mb-3" onChange={e=>{const d=document.querySelector('[name=desc]')as HTMLInputElement;const roles:Record<string,string>={'Developer':'Writes clean, efficient code','Reviewer':'Reviews code for safety and quality','Tester':'Runs tests and reports results','DevOps':'Manages CI/CD and deployment','Analyst':'Analyzes data and creates reports'};if(d)d.value=roles[e.target.value]||'';}}>
            <option value="">Select role</option>
            {['Developer','Reviewer','Tester','DevOps','Analyst'].map(r=><option key={r}>{r}</option>)}
          </select>
          <input name="desc" placeholder="Description" className="w-full px-3 py-2 rounded-lg bg-[#06060a] border border-gray-800 text-sm mb-4"/>
          <button type="submit" className="w-full py-2 bg-indigo-600 rounded-lg text-sm">Create Agent</button>
        </form>
      </Modal>}
      {showAssignTask && <Modal onClose={()=>setShowAssignTask(null)}>
        <h2 className="font-semibold mb-2">Assign to {showAssignTask.name}</h2>
        <p className="text-xs text-gray-500 mb-4">{showAssignTask.role}</p>
        <form onSubmit={async e=>{e.preventDefault();const f=new FormData(e.currentTarget);const r=await fetch(`${API}/api/orgs/${org!.id}/agents/${showAssignTask.id}/tasks`,{method:'POST',headers,body:JSON.stringify({description:f.get('task')})});if(r.ok){setShowAssignTask(null);load();}}}>
          <textarea name="task" placeholder="Describe the task in natural language..." className="w-full px-3 py-2 rounded-lg bg-[#06060a] border border-gray-800 text-sm mb-4 h-24 resize-none" autoFocus/>
          <button type="submit" className="w-full py-2 bg-indigo-600 rounded-lg text-sm">Assign</button>
        </form>
      </Modal>}
      {showSettings && <Modal onClose={()=>setShowSettings(false)}>
        <h2 className="font-semibold mb-4">Model Settings</h2>
        <form onSubmit={async e=>{e.preventDefault();const f=new FormData(e.currentTarget);const r=await fetch(API+'/api/auth/model',{method:'POST',headers,body:JSON.stringify({provider:f.get('provider'),model:f.get('model'),api_key:f.get('key'),temperature:parseFloat(f.get('temp')as string)})});if(r.ok){setShowSettings(false);}}}>
          <label className="block text-[10px] text-gray-500 mb-1">Provider</label>
          <select name="provider" defaultValue={user?.model?.provider||'openai'} className="w-full px-3 py-2 rounded-lg bg-[#06060a] border border-gray-800 text-sm mb-3">
            {['openai','anthropic','deepseek','groq','together','local'].map(p=><option key={p}>{p}</option>)}
          </select>
          <label className="block text-[10px] text-gray-500 mb-1">Model</label>
          <input name="model" defaultValue={user?.model?.model||'gpt-4o'} className="w-full px-3 py-2 rounded-lg bg-[#06060a] border border-gray-800 text-sm mb-3"/>
          <label className="block text-[10px] text-gray-500 mb-1">API Key</label>
          <input name="key" type="password" defaultValue={user?.model?.api_key||''} className="w-full px-3 py-2 rounded-lg bg-[#06060a] border border-gray-800 text-sm mb-3"/>
          <label className="block text-[10px] text-gray-500 mb-1">Temperature: {(user?.model?.temperature||0.7).toFixed(1)}</label>
          <input name="temp" type="range" min="0" max="2" step="0.1" defaultValue={user?.model?.temperature||0.7} className="w-full mb-4 accent-indigo-500"/>
          <button type="submit" className="w-full py-2 bg-indigo-600 rounded-lg text-sm">Save</button>
        </form>
      </Modal>}
    </div>
  );
}

function Modal({children, onClose}:{children:React.ReactNode; onClose:()=>void}) {
  return <div className="fixed inset-0 bg-black/60 flex items-center justify-center z-50" onClick={onClose}>
    <div className="bg-[#0d0d15] border border-gray-800 rounded-xl p-6 w-full max-w-sm mx-4" onClick={e=>e.stopPropagation()}>
      {children}
    </div>
  </div>;
}
