// Agent Platform — top-level App
// Multi-tab workspace: Overview / Agents / Tasks / Activity / Settings
// Toast system + skeleton loading + optimistic UI + WS progress

import './index.css';
import AuthPage from './AuthPage';
import { tt, sl, al, gl } from './i18n';
import { apiBase, wsBase } from './api';
import {
  useState, useEffect, useCallback, useRef,
  createContext, useContext, ReactNode, useMemo,
} from 'react';

// ═══ Theme ═══
const BG = 'linear-gradient(135deg, #05051a 0%, #0a0a2e 30%, #0d0d28 60%, #060622 100%)';
const GRAD = 'linear-gradient(135deg, #6366f1 0%, #8b5cf6 50%, #06b6d4 100%)';
const GRAD_S = 'linear-gradient(135deg, #6366f1, #8b5cf6)';
const TEXT = '#e2e8f0';
const MUTED = 'rgba(148,163,184,0.7)';
const DIM = 'rgba(148,163,184,0.4)';
const CARD = 'rgba(15,15,40,0.7)';
const CARD2 = 'rgba(20,20,55,0.6)';
const BORDER = 'rgba(99,102,241,0.15)';
const BORDER_HI = 'rgba(99,102,241,0.3)';
const INPUT = 'rgba(10,10,25,0.8)';
const SUCCESS = '#22c55e';
const WARN = '#facc15';
const ERROR = '#f87171';
const INFO = '#67e8f9';
const IDLE = '#a5b4fc';

const STATUS_COLOR: Record<string, string> = {
  idle: IDLE, working: WARN, error: ERROR, completed: SUCCESS, pending: IDLE,
  running: WARN, failed: ERROR, success: SUCCESS,
};
const STATUS_BG: Record<string, string> = {
  idle: 'rgba(99,102,241,0.1)', working: 'rgba(250,204,21,0.15)',
  error: 'rgba(248,113,113,0.15)', completed: 'rgba(34,197,94,0.15)',
  pending: 'rgba(99,102,241,0.1)', running: 'rgba(250,204,21,0.15)',
  failed: 'rgba(248,113,113,0.15)', success: 'rgba(34,197,94,0.15)',
};

type T = Record<string, any>;

// ═══ Icons (inline SVG, no library) ═══
const I = {
  Overview: (p: any) => <svg className={p.className||'w-4 h-4'} fill="none" stroke="currentColor" viewBox="0 0 24 24"><path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M3 12l2-2 7-7 9 9-2 2v6a2 2 0 01-2 2h-4v-7H9v7H5a2 2 0 01-2-2v-6z"/></svg>,
  Agents: (p: any) => <svg className={p.className||'w-4 h-4'} fill="none" stroke="currentColor" viewBox="0 0 24 24"><path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M9.75 3.104v5.714a2.25 2.25 0 01-.659 1.591L5 14.5M9.75 3.104c-.251.023-.501.05-.75.082m.75-.082a24.301 24.301 0 014.5 0m0 0v5.714c0 .597.237 1.17.659 1.591L19.8 15.3M14.25 3.104c.251.023.501.05.75.082M19.8 15.3l-1.57.393A9.065 9.065 0 0112 15a9.065 9.065 0 00-6.23.693L5 14.5"/></svg>,
  Tasks: (p: any) => <svg className={p.className||'w-4 h-4'} fill="none" stroke="currentColor" viewBox="0 0 24 24"><path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2m-6 9l2 2 4-4"/></svg>,
  Activity: (p: any) => <svg className={p.className||'w-4 h-4'} fill="none" stroke="currentColor" viewBox="0 0 24 24"><path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M13 10V3L4 14h7v7l9-11h-7z"/></svg>,
  Settings: (p: any) => <svg className={p.className||'w-4 h-4'} fill="none" stroke="currentColor" viewBox="0 0 24 24"><path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z"/><path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M15 12a3 3 0 11-6 0 3 3 0 016 0z"/></svg>,
  Plus: (p: any) => <svg className={p.className||'w-4 h-4'} fill="none" stroke="currentColor" viewBox="0 0 24 24"><path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 4v16m8-8H4"/></svg>,
  Trash: (p: any) => <svg className={p.className||'w-4 h-4'} fill="none" stroke="currentColor" viewBox="0 0 24 24"><path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6M1 7h22M9 7V4a2 2 0 012-2h2a2 2 0 012 2v3"/></svg>,
  X: (p: any) => <svg className={p.className||'w-4 h-4'} fill="none" stroke="currentColor" viewBox="0 0 24 24"><path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12"/></svg>,
  Check: (p: any) => <svg className={p.className||'w-4 h-4'} fill="none" stroke="currentColor" viewBox="0 0 24 24"><path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M5 13l4 4L19 7"/></svg>,
  Alert: (p: any) => <svg className={p.className||'w-4 h-4'} fill="none" stroke="currentColor" viewBox="0 0 24 24"><path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M12 9v2m0 4h.01M5.07 19h13.86c1.54 0 2.5-1.67 1.73-3L13.73 4a2 2 0 00-3.46 0L3.34 16c-.77 1.33.19 3 1.73 3z"/></svg>,
  Info: (p: any) => <svg className={p.className||'w-4 h-4'} fill="none" stroke="currentColor" viewBox="0 0 24 24"><path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M13 16h-1v-4h-1m1-4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"/></svg>,
  Send: (p: any) => <svg className={p.className||'w-4 h-4'} fill="none" stroke="currentColor" viewBox="0 0 24 24"><path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M12 19l9 2-9-18-9 18 9-2zm0 0v-8"/></svg>,
  Sparkle: (p: any) => <svg className={p.className||'w-4 h-4'} fill="none" stroke="currentColor" viewBox="0 0 24 24"><path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M5 3v4M3 5h4M6 17v4m-2-2h4m5-16l2.286 6.857L21 12l-5.714 2.143L13 21l-2.286-6.857L5 12l5.714-2.143L13 3z"/></svg>,
  Spinner: (p: any) => <svg className={`${p.className||'w-4 h-4'} animate-spin`} viewBox="0 0 24 24"><circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" fill="none"/><path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z"/></svg>,
  Chevron: (p: any) => <svg className={p.className||'w-3 h-3'} fill="none" stroke="currentColor" viewBox="0 0 24 24"><path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 9l-7 7-7-7"/></svg>,
  Org: (p: any) => <svg className={p.className||'w-4 h-4'} fill="none" stroke="currentColor" viewBox="0 0 24 24"><path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M19 21V5a2 2 0 00-2-2H7a2 2 0 00-2 2v16m14 0h2m-2 0h-5m-9 0H3m2 0h5M9 7h1m-1 4h1m4-4h1m-1 4h1m-5 10v-5a1 1 0 011-1h2a1 1 0 011 1v5m-4 0h4"/></svg>,
  Bolt: (p: any) => <svg className={p.className||'w-4 h-4'} fill="none" stroke="currentColor" viewBox="0 0 24 24"><path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.8} d="M13 10V3L4 14h7v7l9-11h-7z"/></svg>,
};

// ═══ Toast System ═══
type ToastT = { id: string; type: 'success' | 'error' | 'info' | 'warn'; message: string; title?: string };
const ToastCtx = createContext<{ push: (t: Omit<ToastT, 'id'>) => void }>({ push: () => {} });
const useToast = () => useContext(ToastCtx);

function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<ToastT[]>([]);
  const push = useCallback((t: Omit<ToastT, 'id'>) => {
    const id = Math.random().toString(36).slice(2);
    setToasts(p => [...p, { id, ...t }]);
    setTimeout(() => setToasts(p => p.filter(x => x.id !== id)), 4500);
  }, []);
  const dismiss = (id: string) => setToasts(p => p.filter(x => x.id !== id));
  return <ToastCtx.Provider value={{ push }}>
    {children}
    <div className="fixed top-4 right-4 z-[200] space-y-2 pointer-events-none flex flex-col items-end">
      {toasts.map(t => (
        <div key={t.id} className="pointer-events-auto rounded-xl backdrop-blur-xl border shadow-2xl flex items-start gap-3 px-4 py-3 min-w-[280px] max-w-[420px] animate-in"
          style={{ background: t.type === 'error' ? 'rgba(60,15,15,0.95)' : t.type === 'success' ? 'rgba(10,40,20,0.95)' : t.type === 'warn' ? 'rgba(50,40,10,0.95)' : 'rgba(15,15,45,0.95)',
            borderColor: t.type === 'error' ? 'rgba(248,113,113,0.4)' : t.type === 'success' ? 'rgba(34,197,94,0.4)' : t.type === 'warn' ? 'rgba(250,204,21,0.4)' : 'rgba(99,102,241,0.4)' }}>
          <div className="mt-0.5" style={{ color: t.type === 'error' ? ERROR : t.type === 'success' ? SUCCESS : t.type === 'warn' ? WARN : INFO }}>
            {t.type === 'error' ? <I.Alert className="w-5 h-5"/> : t.type === 'success' ? <I.Check className="w-5 h-5"/> : t.type === 'warn' ? <I.Alert className="w-5 h-5"/> : <I.Info className="w-5 h-5"/>}
          </div>
          <div className="flex-1 min-w-0">
            {t.title && <div className="text-xs font-semibold mb-0.5" style={{color: TEXT}}>{t.title}</div>}
            <div className="text-xs leading-relaxed" style={{color: MUTED}}>{t.message}</div>
          </div>
          <button onClick={() => dismiss(t.id)} className="text-slate-500 hover:text-slate-300 -mt-0.5"><I.X className="w-3.5 h-3.5"/></button>
        </div>
      ))}
    </div>
  </ToastCtx.Provider>;
}

// ═══ Status Badge ═══
function StatusBadge({ status, label }: { status: string; label?: string }) {
  const color = STATUS_COLOR[status] || IDLE;
  const bg = STATUS_BG[status] || 'rgba(99,102,241,0.1)';
  const isWorking = status === 'working' || status === 'running';
  return <span className="px-2 py-0.5 rounded-full text-[10px] font-medium inline-flex items-center gap-1.5" style={{background: bg, color}}>
    <span className="w-1.5 h-1.5 rounded-full" style={{background: color, animation: isWorking ? 'pulse 1.5s infinite' : 'none'}}/>
    {label || status}
  </span>;
}

// ═══ Stat Card ═══
function StatCard({ label, value, hint, color, icon }: { label: string; value: string | number; hint?: string; color?: string; icon?: ReactNode }) {
  return <div className="rounded-2xl p-5 border backdrop-blur-sm transition-all hover:border-opacity-40" style={{background: CARD, borderColor: BORDER}}>
    <div className="flex items-start justify-between mb-3">
      <div className="text-[10px] uppercase tracking-wider" style={{color: MUTED}}>{label}</div>
      {icon && <div style={{color: color || IDLE}}>{icon}</div>}
    </div>
    <div className="text-2xl font-bold" style={{color: color || TEXT}}>{value}</div>
    {hint && <div className="text-[10px] mt-1.5" style={{color: DIM}}>{hint}</div>}
  </div>;
}

// ═══ Skeleton ═══
function Sk({ className = '' }: { className?: string }) {
  return <div className={`rounded animate-pulse ${className}`} style={{background: 'rgba(99,102,241,0.08)'}}/>;
}

// ═══ Empty State ═══
function Empty({ icon, title, hint, action }: { icon: ReactNode; title: string; hint?: string; action?: ReactNode }) {
  return <div className="flex flex-col items-center justify-center py-20 px-6 text-center">
    <div className="w-16 h-16 rounded-2xl flex items-center justify-center mb-4" style={{background: GRAD_S, opacity: 0.4}}>{icon}</div>
    <div className="text-sm font-medium mb-1" style={{color: TEXT}}>{title}</div>
    {hint && <div className="text-xs mb-4 max-w-xs" style={{color: MUTED}}>{hint}</div>}
    {action}
  </div>;
}

// ═══ Modal ═══
function Modal({ onClose, title, subtitle, children, size = 'md' }: { onClose: () => void; title: string; subtitle?: string; children: ReactNode; size?: 'sm' | 'md' | 'lg' }) {
  useEffect(() => {
    const h = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose(); };
    window.addEventListener('keydown', h);
    return () => window.removeEventListener('keydown', h);
  }, [onClose]);
  const maxW = size === 'sm' ? 'max-w-sm' : size === 'lg' ? 'max-w-2xl' : 'max-w-md';
  return <div className="fixed inset-0 z-50 flex items-center justify-center p-4" style={{background: 'rgba(0,0,0,0.7)', backdropFilter: 'blur(8px)'}} onClick={onClose}>
    <div className={`rounded-2xl border w-full ${maxW} shadow-2xl`} style={{background: 'rgba(15,15,40,0.95)', borderColor: BORDER_HI, backdropFilter: 'blur(20px)'}} onClick={e => e.stopPropagation()}>
      <div className="px-6 pt-5 pb-3 flex items-start justify-between border-b" style={{borderColor: BORDER}}>
        <div>
          <h2 className="text-base font-semibold" style={{color: TEXT}}>{title}</h2>
          {subtitle && <p className="text-xs mt-0.5" style={{color: MUTED}}>{subtitle}</p>}
        </div>
        <button onClick={onClose} className="text-slate-500 hover:text-slate-200 transition-colors -mt-1"><I.X className="w-4 h-4"/></button>
      </div>
      <div className="px-6 py-5">{children}</div>
    </div>
  </div>;
}

// ═══ Field ═══
function Field({ name, label, type = 'text', defaultValue = '', placeholder, autoFocus, rows }: { name: string; label: string; type?: string; defaultValue?: string; placeholder?: string; autoFocus?: boolean; rows?: number }) {
  const cls = "w-full px-4 py-3 rounded-xl text-sm outline-none transition-all duration-200 focus:border-opacity-100 placeholder:text-slate-600";
  const style = { background: INPUT, border: '1px solid rgba(99,102,241,0.15)', color: TEXT };
  return <div className="mb-3">
    <label className="block text-[10px] mb-1.5 font-medium uppercase tracking-wider" style={{color: MUTED}}>{label}</label>
    {rows ? <textarea name={name} defaultValue={defaultValue} placeholder={placeholder} rows={rows} autoFocus={autoFocus} className={`${cls} resize-none`} style={style}/>
      : <input name={name} type={type} defaultValue={defaultValue} placeholder={placeholder} autoFocus={autoFocus} className={cls} style={style}/>}
  </div>;
}

// ═══ Submit Button ═══
function Submit({ label, loading, disabled }: { label: string; loading?: boolean; disabled?: boolean }) {
  return <button type="submit" disabled={loading || disabled} className="w-full py-3 rounded-xl text-sm font-semibold transition-all duration-200 disabled:opacity-50 disabled:cursor-not-allowed flex items-center justify-center gap-2 mt-2"
    style={{background: GRAD_S, color: 'white', boxShadow: '0 4px 12px rgba(99,102,241,0.3)'}}>
    {loading && <I.Spinner className="w-4 h-4"/>}
    {loading ? '...' : label}
  </button>;
}

// ═══ Confirm Dialog ═══
function Confirm({ title, message, onConfirm, onCancel, danger }: { title: string; message: string; onConfirm: () => void; onCancel: () => void; danger?: boolean }) {
  return <Modal onClose={onCancel} title={title} size="sm">
    <p className="text-sm mb-5 leading-relaxed" style={{color: MUTED}}>{message}</p>
    <div className="flex gap-2">
      <button onClick={onCancel} className="flex-1 py-2.5 rounded-xl text-sm font-medium transition-colors" style={{background: INPUT, color: TEXT, border: '1px solid rgba(99,102,241,0.15)'}}>Cancel</button>
      <button onClick={onConfirm} className="flex-1 py-2.5 rounded-xl text-sm font-semibold transition-all" style={{background: danger ? 'linear-gradient(135deg, #dc2626, #b91c1c)' : GRAD_S, color: 'white'}}>{danger ? 'Delete' : 'Confirm'}</button>
    </div>
  </Modal>;
}

// ═══ Role Icon ═══
function RoleIcon({ role, className = 'w-5 h-5' }: { role?: string; className?: string }) {
  const colors: Record<string, string> = { Developer: '#60a5fa', Reviewer: '#a78bfa', Tester: '#34d399', DevOps: '#fb923c', Analyst: '#f472b6' };
  const c = colors[role || ''] || IDLE;
  return <div className={`${className} rounded-lg flex items-center justify-center text-xs font-bold`} style={{background: `${c}25`, color: c, border: `1px solid ${c}40`}}>
    {(role || 'AI').slice(0, 2).toUpperCase()}
  </div>;
}

// ═══════════════════════════════════════════════════════════════
// Main App
// ═══════════════════════════════════════════════════════════════

export default function App() {
  return <ToastProvider><MainApp/></ToastProvider>;
}

function MainApp() {
  const [token, setToken] = useState(localStorage.getItem('token') || '');
  if (!token) return <AuthPage onAuth={(t, u) => { setToken(t); localStorage.setItem('token', t); }}/>;

  return <Workspace token={token} onLogout={() => { setToken(''); localStorage.removeItem('token'); }}/>;
}

// ═══════════════════════════════════════════════════════════════
// Workspace (authenticated)
// ═══════════════════════════════════════════════════════════════

type Tab = 'overview' | 'agents' | 'tasks' | 'activity' | 'settings';

function Workspace({ token, onLogout }: { token: string; onLogout: () => void }) {
  const toast = useToast();
  const [user, setUser] = useState<T | null>(null);
  const [orgs, setOrgs] = useState<T[]>([]);
  const [org, setOrg] = useState<T | null>(null);
  const [agents, setAgents] = useState<T[]>([]);
  const [tasks, setTasks] = useState<T[]>([]);
  const [events, setEvents] = useState<string[]>([]);
  const [progress, setProgress] = useState<Record<string, string>>({});
  const [tab, setTab] = useState<Tab>('overview');
  const [showCreateOrg, setShowCreateOrg] = useState(false);
  const [showCreateAgent, setShowCreateAgent] = useState(false);
  const [showAssignTask, setShowAssignTask] = useState<T | null>(null);
  const [showOrgSwitcher, setShowOrgSwitcher] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState<{ type: 'org' | 'agent' | 'task'; id: string; name: string } | null>(null);

  const headers = useMemo(() => ({ Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' }), [token]);
  const url = (p: string) => apiBase() + p;

  // ─── Load identity (me + orgs) ───
  const loadIdentity = useCallback(async () => {
    try {
      const [u, o] = await Promise.all([
        fetch(url('/api/auth/me'), { headers }).then(r => r.ok ? r.json() : null),
        fetch(url('/api/orgs'), { headers }).then(r => r.ok ? r.json() : []),
      ]);
      if (u) setUser(u);
      setOrgs(o);
      setOrg(prev => prev ?? (o.length > 0 ? o[0] : null));
    } catch (e) { console.error('loadIdentity', e); }
  }, [headers]);

  // ─── Load agents + tasks for current org ───
  const loadAgentsTasks = useCallback(async () => {
    if (!org) { setAgents([]); setTasks([]); return; }
    try {
      const [a, t] = await Promise.all([
        fetch(url(`/api/orgs/${org.id}/agents`), { headers }).then(r => r.ok ? r.json() : []),
        fetch(url(`/api/orgs/${org.id}/agents/_/tasks`), { headers }).then(r => r.ok ? r.json() : []),
      ]);
      setAgents(a); setTasks(t);
    } catch (e) { console.error('loadAgentsTasks', e); }
  }, [org, headers]);

  useEffect(() => { loadIdentity(); const i = setInterval(loadIdentity, 6000); return () => clearInterval(i); }, [loadIdentity]);
  useEffect(() => { loadAgentsTasks(); const i = setInterval(loadAgentsTasks, 5000); return () => clearInterval(i); }, [loadAgentsTasks]);

  // ─── WebSocket for live progress ───
  useEffect(() => {
    let ws: WebSocket | null = null;
    let retry = 0;
    let cancelled = false;
    let timer: number | null = null;
    const connect = () => {
      if (cancelled) return;
      try {
        ws = new WebSocket(wsBase() + '/api/ws');
      } catch { scheduleRetry(); return; }
      ws.onopen = () => { retry = 0; };
      ws.onmessage = e => {
        const data = e.data;
        setEvents(p => [...p.slice(-100), data]);
        if (data.startsWith('progress:')) {
          const [, aid, ...r] = data.split(':');
          setProgress(p => ({ ...p, [aid]: r.join(':') }));
        } else if (data.startsWith('done:') || data.startsWith('fail:') || data.startsWith('task:')) {
          setProgress({});
          loadAgentsTasks();
        }
      };
      ws.onerror = () => {};
      ws.onclose = () => { if (!cancelled) scheduleRetry(); };
    };
    const scheduleRetry = () => {
      retry = Math.min(retry + 1, 6);
      const delay = Math.min(1000 * (2 ** (retry - 1)), 15000);
      timer = window.setTimeout(connect, delay);
    };
    connect();
    return () => { cancelled = true; if (timer) clearTimeout(timer); if (ws) ws.close(); };
  }, [loadAgentsTasks]);

  // ─── Actions: create org ───
  const onCreateOrg = async (data: { name: string; description: string }) => {
    try {
      const r = await fetch(url('/api/orgs'), { method: 'POST', headers, body: JSON.stringify(data) });
      if (!r.ok) {
        const e = await r.json().catch(() => ({}));
        toast.push({ type: 'error', title: 'Create organization failed', message: e.error || `Server error (${r.status})` });
        return false;
      }
      const newOrg = await r.json();
      setOrg(newOrg);
      setOrgs(p => [newOrg, ...p]);
      toast.push({ type: 'success', title: 'Organization created', message: `"${newOrg.name}" is ready` });
      return true;
    } catch (e: any) {
      toast.push({ type: 'error', title: 'Network error', message: e.message || 'Could not reach backend' });
      return false;
    }
  };

  // ─── Actions: create agent ───
  const onCreateAgent = async (data: { name: string; role: string; description: string }) => {
    if (!org) return false;
    try {
      const r = await fetch(url(`/api/orgs/${org.id}/agents`), { method: 'POST', headers, body: JSON.stringify(data) });
      if (!r.ok) {
        const e = await r.json().catch(() => ({}));
        toast.push({ type: 'error', title: 'Create agent failed', message: e.error || `Server error (${r.status})` });
        return false;
      }
      const newA = await r.json();
      setAgents(p => [newA, ...p]);
      toast.push({ type: 'success', title: 'Agent created', message: `${newA.name} (${newA.role}) added to team` });
      return true;
    } catch (e: any) {
      toast.push({ type: 'error', title: 'Network error', message: e.message });
      return false;
    }
  };

  // ─── Actions: assign task ───
  const onAssignTask = async (agentId: string, description: string) => {
    if (!org) return false;
    try {
      const r = await fetch(url(`/api/orgs/${org.id}/agents/${agentId}/tasks`), { method: 'POST', headers, body: JSON.stringify({ description }) });
      if (!r.ok) {
        const e = await r.json().catch(() => ({}));
        toast.push({ type: 'error', title: 'Assign task failed', message: e.error || `Server error (${r.status})` });
        return false;
      }
      const newT = await r.json();
      setTasks(p => [newT, ...p]);
      toast.push({ type: 'success', title: 'Task assigned', message: `"${description.slice(0, 50)}${description.length > 50 ? '...' : ''}"` });
      loadAgentsTasks();
      return true;
    } catch (e: any) {
      toast.push({ type: 'error', title: 'Network error', message: e.message });
      return false;
    }
  };

  // ─── Actions: delete org/agent/task ───
  const doDelete = async () => {
    if (!confirmDelete) return;
    const { type, id, name } = confirmDelete;
    setConfirmDelete(null);
    try {
      let endpoint = '';
      if (type === 'org') endpoint = `/api/orgs/${id}`;
      else if (type === 'agent' && org) endpoint = `/api/orgs/${org.id}/agents/${id}`;
      else if (type === 'task' && org) {
        // For now just a stub - we don't have delete task yet, hide
        toast.push({ type: 'info', message: 'Task deletion coming soon' });
        return;
      }
      if (!endpoint) return;
      const r = await fetch(url(endpoint), { method: 'DELETE', headers });
      if (!r.ok && r.status !== 204) {
        const e = await r.json().catch(() => ({}));
        toast.push({ type: 'error', title: 'Delete failed', message: e.error || `Server error (${r.status})` });
        return;
      }
      // Update local state
      if (type === 'org') {
        setOrgs(p => p.filter(x => x.id !== id));
        if (org?.id === id) { setOrg(null); setAgents([]); setTasks([]); }
        toast.push({ type: 'success', title: 'Organization deleted', message: `"${name}" removed` });
      } else if (type === 'agent') {
        setAgents(p => p.filter(x => x.id !== id));
        toast.push({ type: 'success', title: 'Agent removed', message: `"${name}" removed from team` });
      }
    } catch (e: any) {
      toast.push({ type: 'error', title: 'Network error', message: e.message });
    }
  };

  // ─── Computed: org stats ───
  const stats = useMemo(() => {
    const totalTasks = tasks.length;
    const completed = tasks.filter(t => t.status === 'completed').length;
    const running = tasks.filter(t => t.status === 'running').length;
    const successRate = totalTasks > 0 ? Math.round(completed / totalTasks * 100) : 0;
    return { totalTasks, completed, running, successRate };
  }, [tasks]);

  return (
    <div className="min-h-screen flex flex-col" style={{background: BG, color: TEXT}}>
      {/* ══════ Top Bar ══════ */}
      <div className="h-16 flex items-center justify-between px-6 border-b backdrop-blur-xl" style={{background: 'rgba(10,10,30,0.7)', borderColor: BORDER}}>
        <div className="flex items-center gap-3">
          <div className="w-9 h-9 rounded-xl flex items-center justify-center" style={{background: GRAD}}>
            <I.Agents className="w-5 h-5 text-white"/>
          </div>
          <div>
            <div className="text-sm font-bold tracking-tight" style={{background: GRAD, WebkitBackgroundClip: 'text', WebkitTextFillColor: 'transparent'}}>HyperAgent Platform</div>
            <div className="text-[10px]" style={{color: DIM}}>Multi-agent orchestration</div>
          </div>
        </div>

        {/* Org switcher */}
        {org && (
          <button onClick={() => setShowOrgSwitcher(true)} className="flex items-center gap-2 px-4 py-1.5 rounded-xl transition-all hover:border-opacity-40" style={{background: CARD, border: '1px solid ' + BORDER}}>
            <I.Org className="w-3.5 h-3.5" style={{color: IDLE}}/>
            <span className="text-xs font-medium">{org.name}</span>
            <I.Chevron className="w-3 h-3" style={{color: MUTED}}/>
          </button>
        )}

        <div className="flex items-center gap-3">
          <select value={gl()} onChange={e => sl(e.target.value)} className="text-[11px] py-1.5 px-2.5 rounded-lg border appearance-none cursor-pointer" style={{background: INPUT, borderColor: BORDER, color: MUTED}}>
            {al().map(l => <option key={l.code} value={l.code}>{l.name}</option>)}
          </select>
          <div className="text-xs px-3 py-1.5 rounded-lg" style={{background: CARD, color: MUTED, border: '1px solid ' + BORDER}}>
            {user?.email}
          </div>
          <button onClick={onLogout} className="text-xs px-3 py-1.5 rounded-lg transition-colors hover:bg-white/5" style={{color: MUTED}}>{tt('logout')}</button>
        </div>
      </div>

      {/* ══════ Body ══════ */}
      <div className="flex-1 flex">
        {/* ── Sidebar ── */}
        <div className="w-60 flex flex-col p-3 border-r" style={{background: 'rgba(10,10,30,0.5)', borderColor: BORDER}}>
          <nav className="space-y-1 mb-4">
            {([
              { k: 'overview', l: tt('overview') || 'Overview', i: <I.Overview className="w-4 h-4"/> },
              { k: 'agents', l: tt('agents') || 'Agents', i: <I.Agents className="w-4 h-4"/> },
              { k: 'tasks', l: tt('tasks') || 'Tasks', i: <I.Tasks className="w-4 h-4"/> },
              { k: 'activity', l: tt('activity') || 'Activity', i: <I.Activity className="w-4 h-4"/> },
              { k: 'settings', l: tt('settings'), i: <I.Settings className="w-4 h-4"/> },
            ] as { k: Tab; l: string; i: ReactNode }[]).map(t => (
              <button key={t.k} onClick={() => setTab(t.k)} className="w-full flex items-center gap-3 px-3 py-2 rounded-lg text-xs font-medium transition-all duration-200"
                style={tab === t.k ? {background: 'rgba(99,102,241,0.15)', color: '#a5b4fc', border: '1px solid rgba(99,102,241,0.25)'} : {color: MUTED, border: '1px solid transparent'}}>
                {t.i}<span className="flex-1 text-left">{t.l}</span>
                {t.k === 'tasks' && stats.running > 0 && <span className="text-[9px] px-1.5 py-0.5 rounded-full" style={{background: 'rgba(250,204,21,0.2)', color: WARN}}>{stats.running}</span>}
                {t.k === 'agents' && agents.length > 0 && <span className="text-[9px] px-1.5 py-0.5 rounded-full" style={{background: 'rgba(99,102,241,0.15)', color: IDLE}}>{agents.length}</span>}
              </button>
            ))}
          </nav>

          <div className="text-[10px] uppercase tracking-wider mb-2 px-3 mt-2" style={{color: DIM}}>{tt('orgs')}</div>
          <div className="flex-1 overflow-y-auto space-y-1 mb-2">
            {orgs.length === 0 && <div className="text-[10px] px-3 py-2" style={{color: DIM}}>No organizations yet</div>}
            {orgs.map(o => (
              <button key={o.id} onClick={() => { setOrg(o); setAgents([]); setTasks([]); setTab('overview'); loadAgentsTasks(); }} className="w-full text-left px-3 py-2 rounded-lg text-xs flex items-center gap-2 transition-all"
                style={org?.id === o.id ? {background: 'rgba(99,102,241,0.12)', color: TEXT, border: '1px solid rgba(99,102,241,0.2)'} : {color: MUTED, border: '1px solid transparent'}}>
                <I.Org className="w-3 h-3 shrink-0"/>
                <span className="truncate flex-1">{o.name}</span>
              </button>
            ))}
          </div>
          <button onClick={() => setShowCreateOrg(true)} className="w-full flex items-center justify-center gap-2 py-2 border border-dashed rounded-lg text-xs transition-colors hover:border-opacity-60" style={{borderColor: BORDER, color: MUTED}}>
            <I.Plus className="w-3 h-3"/>{tt('newOrg')}
          </button>
        </div>

        {/* ── Main content ── */}
        <div className="flex-1 overflow-auto p-8">
          {!org ? (
            <Empty icon={<I.Org className="w-7 h-7 text-white"/>} title="Create your first organization" hint="An organization is a workspace where your agents and tasks live. You can create multiple organizations to separate teams or projects."
              action={<button onClick={() => setShowCreateOrg(true)} className="px-6 py-2.5 rounded-xl text-sm font-semibold inline-flex items-center gap-2" style={{background: GRAD_S, color: 'white'}}>
                <I.Plus className="w-4 h-4"/>Create organization
              </button>}/>
          ) : (
            <>
              {tab === 'overview' && <OverviewView org={org} stats={stats} agents={agents} tasks={tasks} events={events} progress={progress} onCreateAgent={() => setShowCreateAgent(true)} onAssignTask={a => setShowAssignTask(a)} onTab={setTab}/>}
              {tab === 'agents' && <AgentsView org={org} agents={agents} tasks={tasks} progress={progress} onCreateAgent={() => setShowCreateAgent(true)} onAssignTask={a => setShowAssignTask(a)} onDelete={a => setConfirmDelete({ type: 'agent', id: a.id, name: a.name })}/>}
              {tab === 'tasks' && <TasksView org={org} agents={agents} tasks={tasks} onAssign={() => agents[0] && setShowAssignTask(agents[0])}/>}
              {tab === 'activity' && <ActivityView events={events} agents={agents}/>}
              {tab === 'settings' && <SettingsView user={user} headers={headers} url={url} toast={toast}/>}
            </>
          )}
        </div>
      </div>

      {/* ══════ Modals ══════ */}
      {showCreateOrg && <CreateOrgModal onClose={() => setShowCreateOrg(false)} onSubmit={onCreateOrg}/>}
      {showCreateAgent && org && <CreateAgentModal org={org} onClose={() => setShowCreateAgent(false)} onSubmit={onCreateAgent}/>}
      {showAssignTask && org && <AssignTaskModal org={org} agent={showAssignTask} onClose={() => setShowAssignTask(null)} onSubmit={d => onAssignTask(showAssignTask.id, d.description)}/>}
      {showOrgSwitcher && <OrgSwitcher orgs={orgs} current={org} onSelect={o => { setOrg(o); setAgents([]); setTasks([]); setShowOrgSwitcher(false); loadAgentsTasks(); }} onCreate={() => { setShowOrgSwitcher(false); setShowCreateOrg(true); }} onClose={() => setShowOrgSwitcher(false)}/>}
      {confirmDelete && <Confirm title={`Delete ${confirmDelete.type}?`} message={`Are you sure you want to delete "${confirmDelete.name}"? This action cannot be undone.`} danger onConfirm={doDelete} onCancel={() => setConfirmDelete(null)}/>}

      {/* Global keyframe styles (used for toasts, pulse, etc.) */}
      <style>{`
        @keyframes pulse { 0%, 100% { opacity: 1 } 50% { opacity: 0.4 } }
        @keyframes slide-in { from { transform: translateX(20px); opacity: 0 } to { transform: translateX(0); opacity: 1 } }
        .animate-in { animation: slide-in 0.25s ease-out }
        ::-webkit-scrollbar { width: 8px; height: 8px }
        ::-webkit-scrollbar-track { background: transparent }
        ::-webkit-scrollbar-thumb { background: rgba(99,102,241,0.2); border-radius: 4px }
        ::-webkit-scrollbar-thumb:hover { background: rgba(99,102,241,0.35) }
      `}</style>
    </div>
  );
}

// ═══════════════════════════════════════════════════════════════
// Overview View
// ═══════════════════════════════════════════════════════════════
function OverviewView({ org, stats, agents, tasks, events, progress, onCreateAgent, onAssignTask, onTab }: { org: T; stats: { totalTasks: number; completed: number; running: number; successRate: number }; agents: T[]; tasks: T[]; events: string[]; progress: Record<string, string>; onCreateAgent: () => void; onAssignTask: (a: T) => void; onTab: (t: Tab) => void }) {
  return <div>
    {/* Header */}
    <div className="mb-6 flex items-start justify-between">
      <div>
        <h1 className="text-2xl font-bold tracking-tight">{org.name}</h1>
        <p className="text-sm mt-1" style={{color: MUTED}}>{org.description || 'No description set'}</p>
      </div>
      <button onClick={onCreateAgent} className="px-5 py-2.5 rounded-xl text-sm font-semibold inline-flex items-center gap-2 transition-all hover:shadow-lg active:scale-[0.98]" style={{background: GRAD_S, color: 'white', boxShadow: '0 4px 12px rgba(99,102,241,0.3)'}}>
        <I.Plus className="w-4 h-4"/>New Agent
      </button>
    </div>

    {/* Stat cards */}
    <div className="grid grid-cols-4 gap-4 mb-6">
      <StatCard label="Agents" value={agents.length} hint={agents.length === 0 ? 'No agents yet' : `${agents.filter(a => a.status === 'working' || progress[a.id]).length} active now`} color={IDLE} icon={<I.Agents className="w-5 h-5"/>}/>
      <StatCard label="Tasks" value={stats.totalTasks} hint={stats.running > 0 ? `${stats.running} running` : 'All queued'} color={WARN} icon={<I.Tasks className="w-5 h-5"/>}/>
      <StatCard label="Completed" value={stats.completed} hint={stats.totalTasks > 0 ? `${stats.totalTasks - stats.completed} pending` : 'No tasks yet'} color={SUCCESS} icon={<I.Check className="w-5 h-5"/>}/>
      <StatCard label="Success rate" value={`${stats.successRate}%`} hint={stats.totalTasks > 0 ? 'Across all agents' : 'No data'} color={stats.successRate >= 80 ? SUCCESS : stats.successRate >= 50 ? WARN : ERROR} icon={<I.Bolt className="w-5 h-5"/>}/>
    </div>

    {/* Two columns: Recent agents + Recent activity */}
    <div className="grid grid-cols-3 gap-4">
      <div className="col-span-2 rounded-2xl p-5 border" style={{background: CARD, borderColor: BORDER}}>
        <div className="flex items-center justify-between mb-4">
          <h3 className="text-sm font-semibold">Agents</h3>
          <button onClick={() => onTab('agents')} className="text-xs" style={{color: IDLE}}>View all →</button>
        </div>
        {agents.length === 0 ? (
          <Empty icon={<I.Agents className="w-7 h-7 text-white"/>} title="No agents yet" hint="Create your first agent to start automating tasks." action={
            <button onClick={onCreateAgent} className="px-5 py-2 rounded-xl text-sm font-semibold inline-flex items-center gap-2" style={{background: GRAD_S, color: 'white'}}><I.Plus className="w-4 h-4"/>New Agent</button>
          }/>
        ) : (
          <div className="space-y-2">
            {agents.slice(0, 4).map(a => <div key={a.id} className="flex items-center gap-3 p-3 rounded-xl transition-all hover:bg-white/[0.03]">
              <RoleIcon role={a.role} className="w-10 h-10"/>
              <div className="flex-1 min-w-0">
                <div className="text-sm font-medium truncate">{a.name}</div>
                <div className="text-xs" style={{color: MUTED}}>{a.role} · {a.total_tasks || 0} tasks</div>
              </div>
              <StatusBadge status={progress[a.id] ? 'working' : (a.status || 'idle')}/>
              <button onClick={() => onAssignTask(a)} className="text-xs px-3 py-1.5 rounded-lg transition-colors" style={{background: INPUT, color: MUTED, border: '1px solid ' + BORDER}}>{tt('assignTask')}</button>
            </div>)}
          </div>
        )}
      </div>

      <div className="rounded-2xl p-5 border" style={{background: CARD, borderColor: BORDER}}>
        <div className="flex items-center justify-between mb-4">
          <h3 className="text-sm font-semibold">Live activity</h3>
          <button onClick={() => onTab('activity')} className="text-xs" style={{color: IDLE}}>View all →</button>
        </div>
        <div className="space-y-2 max-h-72 overflow-y-auto">
          {events.length === 0 ? (
            <div className="text-xs text-center py-8" style={{color: DIM}}>Waiting for events…</div>
          ) : events.slice(-10).reverse().map((e, i) => <EventRow key={i} event={e} agents={agents}/>)}
        </div>
      </div>
    </div>
  </div>;
}

// ═══════════════════════════════════════════════════════════════
// Agents View
// ═══════════════════════════════════════════════════════════════
function AgentsView({ org, agents, tasks, progress, onCreateAgent, onAssignTask, onDelete }: { org: T; agents: T[]; tasks: T[]; progress: Record<string, string>; onCreateAgent: () => void; onAssignTask: (a: T) => void; onDelete: (a: T) => void }) {
  return <div>
    <div className="flex items-center justify-between mb-6">
      <div>
        <h1 className="text-2xl font-bold tracking-tight">Agents</h1>
        <p className="text-sm mt-1" style={{color: MUTED}}>{agents.length} agent{agents.length !== 1 ? 's' : ''} in this organization</p>
      </div>
      <button onClick={onCreateAgent} className="px-5 py-2.5 rounded-xl text-sm font-semibold inline-flex items-center gap-2" style={{background: GRAD_S, color: 'white'}}>
        <I.Plus className="w-4 h-4"/>New Agent
      </button>
    </div>

    {agents.length === 0 ? (
      <Empty icon={<I.Agents className="w-7 h-7 text-white"/>} title="No agents yet" hint="Agents are autonomous workers that can execute tasks, review code, run tests, and more. Create one to get started."
        action={<button onClick={onCreateAgent} className="px-5 py-2.5 rounded-xl text-sm font-semibold inline-flex items-center gap-2" style={{background: GRAD_S, color: 'white'}}><I.Plus className="w-4 h-4"/>New Agent</button>}/>
    ) : (
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        {agents.map(a => <AgentCard key={a.id} agent={a} tasks={tasks.filter(t => t.agent_id === a.id)} progressMsg={progress[a.id]} onAssign={() => onAssignTask(a)} onDelete={() => onDelete(a)}/>)}
      </div>
    )}
  </div>;
}

function AgentCard({ agent, tasks, progressMsg, onAssign, onDelete }: { agent: T; tasks: T[]; progressMsg?: string; onAssign: () => void; onDelete: () => void }) {
  const total = agent.total_tasks || 0;
  const done = agent.completed_tasks || 0;
  const rate = total > 0 ? Math.round(done / total * 100) : 0;
  const status = progressMsg ? 'working' : (agent.status || 'idle');
  return <div className="rounded-2xl p-5 border transition-all hover:border-opacity-50 group" style={{background: CARD, borderColor: BORDER}}>
    <div className="flex items-start justify-between mb-3">
      <div className="flex items-center gap-3 min-w-0 flex-1">
        <RoleIcon role={agent.role} className="w-11 h-11"/>
        <div className="min-w-0 flex-1">
          <h3 className="font-semibold text-sm truncate">{agent.name}</h3>
          <p className="text-xs" style={{color: IDLE}}>{agent.role}</p>
        </div>
      </div>
      <StatusBadge status={status}/>
    </div>
    <p className="text-xs mb-4 line-clamp-2 min-h-[2rem]" style={{color: MUTED}}>{agent.description || 'No description'}</p>

    {progressMsg && <div className="mb-4 flex items-center gap-2 px-3 py-2 rounded-lg" style={{background: 'rgba(250,204,21,0.1)', border: '1px solid rgba(250,204,21,0.2)'}}>
      <I.Spinner className="w-3 h-3" style={{color: WARN}}/>
      <span className="text-[11px]" style={{color: WARN}}>{progressMsg}</span>
    </div>}

    <div className="grid grid-cols-3 gap-3 mb-4">
      <div className="text-center"><div className="text-base font-bold">{total}</div><div className="text-[10px] uppercase tracking-wider" style={{color: DIM}}>Total</div></div>
      <div className="text-center"><div className="text-base font-bold" style={{color: SUCCESS}}>{done}</div><div className="text-[10px] uppercase tracking-wider" style={{color: DIM}}>Done</div></div>
      <div className="text-center"><div className="text-base font-bold" style={{color: rate >= 80 ? SUCCESS : rate >= 50 ? WARN : ERROR}}>{rate}%</div><div className="text-[10px] uppercase tracking-wider" style={{color: DIM}}>Rate</div></div>
    </div>

    {tasks.length > 0 && <div className="space-y-1.5 mb-4 pt-3 border-t" style={{borderColor: BORDER}}>
      {tasks.slice(-3).map(t => <div key={t.id} className="flex items-start gap-1.5 text-[10px]">
        <span className="shrink-0 mt-0.5">{t.status === 'completed' ? '✅' : t.status === 'running' ? '🔄' : t.status === 'failed' ? '❌' : '⏳'}</span>
        <span className="truncate flex-1" style={{color: MUTED}} title={t.description}>{t.description}</span>
      </div>)}
    </div>}

    <div className="flex gap-2">
      <button onClick={onAssign} className="flex-1 py-2 rounded-lg text-xs font-semibold transition-all inline-flex items-center justify-center gap-1.5" style={{background: GRAD_S, color: 'white'}}>
        <I.Send className="w-3 h-3"/>{tt('assignTask')}
      </button>
      <button onClick={onDelete} className="px-2.5 rounded-lg transition-colors opacity-0 group-hover:opacity-100" style={{background: 'rgba(248,113,113,0.1)', color: ERROR, border: '1px solid rgba(248,113,113,0.2)'}} title="Delete agent">
        <I.Trash className="w-3.5 h-3.5"/>
      </button>
    </div>
  </div>;
}

// ═══════════════════════════════════════════════════════════════
// Tasks View
// ═══════════════════════════════════════════════════════════════
function TasksView({ org, agents, tasks, onAssign }: { org: T; agents: T[]; tasks: T[]; onAssign: () => void }) {
  const [filter, setFilter] = useState<'all' | 'pending' | 'running' | 'completed' | 'failed'>('all');
  const filtered = filter === 'all' ? tasks : tasks.filter(t => t.status === filter);
  const stats = {
    all: tasks.length,
    pending: tasks.filter(t => t.status === 'pending').length,
    running: tasks.filter(t => t.status === 'running').length,
    completed: tasks.filter(t => t.status === 'completed').length,
    failed: tasks.filter(t => t.status === 'failed').length,
  };
  return <div>
    <div className="flex items-center justify-between mb-6">
      <div>
        <h1 className="text-2xl font-bold tracking-tight">Tasks</h1>
        <p className="text-sm mt-1" style={{color: MUTED}}>{tasks.length} task{tasks.length !== 1 ? 's' : ''} in this organization</p>
      </div>
      {agents.length > 0 && <button onClick={onAssign} className="px-5 py-2.5 rounded-xl text-sm font-semibold inline-flex items-center gap-2" style={{background: GRAD_S, color: 'white'}}>
        <I.Plus className="w-4 h-4"/>New Task
      </button>}
    </div>

    {/* Filter tabs */}
    <div className="flex gap-1 mb-4 p-1 rounded-xl" style={{background: CARD, border: '1px solid ' + BORDER, width: 'fit-content'}}>
      {(['all', 'pending', 'running', 'completed', 'failed'] as const).map(f => (
        <button key={f} onClick={() => setFilter(f)} className="px-3 py-1.5 rounded-lg text-xs font-medium transition-all capitalize"
          style={filter === f ? {background: GRAD_S, color: 'white'} : {color: MUTED}}>
          {f} <span className="ml-1 opacity-60">{stats[f]}</span>
        </button>
      ))}
    </div>

    {filtered.length === 0 ? (
      <Empty icon={<I.Tasks className="w-7 h-7 text-white"/>} title={filter === 'all' ? 'No tasks yet' : `No ${filter} tasks`} hint={filter === 'all' ? 'Assign your first task to an agent to see it here.' : 'Try a different filter.'} action={
        filter === 'all' && agents.length > 0 ? <button onClick={onAssign} className="px-5 py-2.5 rounded-xl text-sm font-semibold inline-flex items-center gap-2" style={{background: GRAD_S, color: 'white'}}><I.Plus className="w-4 h-4"/>New Task</button> : null
      }/>
    ) : (
      <div className="rounded-2xl border overflow-hidden" style={{background: CARD, borderColor: BORDER}}>
        {filtered.map((t, i) => <TaskRow key={t.id} task={t} agents={agents} isLast={i === filtered.length - 1}/>)}
      </div>
    )}
  </div>;
}

function TaskRow({ task, agents, isLast }: { task: T; agents: T[]; isLast: boolean }) {
  const agent = agents.find(a => a.id === task.agent_id);
  const icon = task.status === 'completed' ? '✅' : task.status === 'running' ? '🔄' : task.status === 'failed' ? '❌' : task.status === 'pending' ? '⏳' : '•';
  const elapsed = task.duration_ms ? `${(task.duration_ms / 1000).toFixed(1)}s` : null;
  return <div className="px-5 py-4 flex items-start gap-4 hover:bg-white/[0.02] transition-colors" style={{borderBottom: isLast ? 'none' : '1px solid ' + BORDER}}>
    <div className="text-lg mt-0.5">{icon}</div>
    <div className="flex-1 min-w-0">
      <div className="flex items-center gap-2 mb-1">
        <span className="text-sm font-medium">{task.description}</span>
        <StatusBadge status={task.status}/>
      </div>
      <div className="flex items-center gap-3 text-xs" style={{color: MUTED}}>
        {agent && <span className="flex items-center gap-1.5"><RoleIcon role={agent.role} className="w-4 h-4 text-[9px]"/>{agent.name}</span>}
        <span>·</span>
        <span>{new Date(task.created_at).toLocaleString()}</span>
        {elapsed && <><span>·</span><span style={{color: SUCCESS}}>{elapsed}</span></>}
      </div>
      {task.result && <div className="mt-2 px-3 py-2 rounded-lg text-xs" style={{background: 'rgba(34,197,94,0.08)', color: 'rgba(34,197,94,0.9)', border: '1px solid rgba(34,197,94,0.2)'}}>
        {task.result}
      </div>}
    </div>
  </div>;
}

// ═══════════════════════════════════════════════════════════════
// Activity View
// ═══════════════════════════════════════════════════════════════
function ActivityView({ events, agents }: { events: string[]; agents: T[] }) {
  return <div>
    <div className="mb-6">
      <h1 className="text-2xl font-bold tracking-tight">Activity</h1>
      <p className="text-sm mt-1" style={{color: MUTED}}>Real-time stream of events from all agents</p>
    </div>
    {events.length === 0 ? (
      <Empty icon={<I.Activity className="w-7 h-7 text-white"/>} title="No activity yet" hint="When agents work on tasks, you'll see their progress and completion events here in real-time."/>
    ) : (
      <div className="rounded-2xl border p-4" style={{background: CARD, borderColor: BORDER}}>
        <div className="space-y-1 max-h-[60vh] overflow-y-auto">
          {events.slice().reverse().map((e, i) => <EventRow key={i} event={e} agents={agents} showTime/>)}
        </div>
      </div>
    )}
  </div>;
}

function EventRow({ event, agents, showTime }: { event: string; agents: T[]; showTime?: boolean }) {
  const time = new Date().toLocaleTimeString();
  let icon = '•'; let color = MUTED;
  if (event.startsWith('progress:')) { icon = '⟳'; color = WARN; }
  else if (event.startsWith('done:')) { icon = '✓'; color = SUCCESS; }
  else if (event.startsWith('fail:')) { icon = '✗'; color = ERROR; }
  else if (event.startsWith('task:')) { icon = '⚡'; color = IDLE; }
  else if (event.startsWith('org:') || event.startsWith('agent:')) { icon = '◆'; color = INFO; }
  // extract agent id if any
  const m = event.match(/^(?:progress|task|done|fail):(\w+)/);
  const agent = m ? agents.find(a => a.id === m[1]) : null;
  return <div className="flex items-center gap-2 text-xs px-2 py-1.5 rounded-lg hover:bg-white/[0.02]">
    <span style={{color}} className="w-3 text-center font-mono">{icon}</span>
    {agent && <RoleIcon role={agent.role} className="w-4 h-4 text-[8px]"/>}
    <span className="font-mono text-[11px] flex-1 truncate" style={{color: TEXT}}>{event}</span>
    {showTime && <span className="text-[10px] font-mono" style={{color: DIM}}>{time}</span>}
  </div>;
}

// ═══════════════════════════════════════════════════════════════
// Settings View
// ═══════════════════════════════════════════════════════════════
function SettingsView({ user, headers, url, toast }: { user: T | null; headers: any; url: (p: string) => string; toast: { push: (t: any) => void } }) {
  const [provider, setProvider] = useState(user?.model?.provider || 'openai');
  const [model, setModel] = useState(user?.model?.model || 'gpt-4o');
  const [apiKey, setApiKey] = useState(user?.model?.api_key || '');
  const [temp, setTemp] = useState(user?.model?.temperature || 0.7);
  const [saving, setSaving] = useState(false);

  const save = async () => {
    setSaving(true);
    try {
      const r = await fetch(url('/api/auth/model'), { method: 'POST', headers, body: JSON.stringify({ provider, model, api_key: apiKey, temperature: temp }) });
      if (!r.ok) { const e = await r.json().catch(() => ({})); toast.push({ type: 'error', title: 'Save failed', message: e.error || `Server error (${r.status})` }); }
      else { toast.push({ type: 'success', title: 'Settings saved', message: 'Your model configuration has been updated' }); }
    } catch (e: any) { toast.push({ type: 'error', title: 'Network error', message: e.message }); }
    finally { setSaving(false); }
  };

  return <div>
    <div className="mb-6">
      <h1 className="text-2xl font-bold tracking-tight">Settings</h1>
      <p className="text-sm mt-1" style={{color: MUTED}}>Manage your profile and model configuration</p>
    </div>

    <div className="grid grid-cols-1 lg:grid-cols-2 gap-4 max-w-4xl">
      {/* Profile */}
      <div className="rounded-2xl p-5 border" style={{background: CARD, borderColor: BORDER}}>
        <h3 className="text-sm font-semibold mb-4">Profile</h3>
        <div className="space-y-3">
          <div><div className="text-[10px] uppercase tracking-wider mb-1" style={{color: DIM}}>Email</div><div className="text-sm">{user?.email}</div></div>
          <div><div className="text-[10px] uppercase tracking-wider mb-1" style={{color: DIM}}>Name</div><div className="text-sm">{user?.name}</div></div>
          <div><div className="text-[10px] uppercase tracking-wider mb-1" style={{color: DIM}}>Member since</div><div className="text-sm">{user?.created_at ? new Date(user.created_at).toLocaleDateString() : '—'}</div></div>
        </div>
      </div>

      {/* Model config */}
      <div className="rounded-2xl p-5 border" style={{background: CARD, borderColor: BORDER}}>
        <h3 className="text-sm font-semibold mb-4">Default model</h3>
        <div className="space-y-3">
          <div>
            <label className="block text-[10px] uppercase tracking-wider mb-1.5" style={{color: MUTED}}>Provider</label>
            <select value={provider} onChange={e => setProvider(e.target.value)} className="w-full px-3 py-2.5 rounded-xl text-sm outline-none" style={{background: INPUT, border: '1px solid ' + BORDER, color: TEXT}}>
              {['openai', 'anthropic', 'deepseek', 'groq', 'together', 'local'].map(p => <option key={p} value={p}>{p}</option>)}
            </select>
          </div>
          <div>
            <label className="block text-[10px] uppercase tracking-wider mb-1.5" style={{color: MUTED}}>Model</label>
            <input value={model} onChange={e => setModel(e.target.value)} className="w-full px-3 py-2.5 rounded-xl text-sm outline-none" style={{background: INPUT, border: '1px solid ' + BORDER, color: TEXT}} placeholder="gpt-4o"/>
          </div>
          <div>
            <label className="block text-[10px] uppercase tracking-wider mb-1.5" style={{color: MUTED}}>API key</label>
            <input value={apiKey} onChange={e => setApiKey(e.target.value)} type="password" className="w-full px-3 py-2.5 rounded-xl text-sm outline-none font-mono" style={{background: INPUT, border: '1px solid ' + BORDER, color: TEXT}} placeholder="sk-..."/>
          </div>
          <div>
            <label className="block text-[10px] uppercase tracking-wider mb-1.5" style={{color: MUTED}}>Temperature: {temp.toFixed(1)}</label>
            <input type="range" min="0" max="2" step="0.1" value={temp} onChange={e => setTemp(parseFloat(e.target.value))} className="w-full accent-indigo-500"/>
          </div>
          <button onClick={save} disabled={saving} className="w-full py-2.5 rounded-xl text-sm font-semibold transition-all inline-flex items-center justify-center gap-2 disabled:opacity-50" style={{background: GRAD_S, color: 'white'}}>
            {saving ? <I.Spinner className="w-4 h-4"/> : null}{saving ? 'Saving…' : 'Save changes'}
          </button>
        </div>
      </div>

      {/* Danger zone */}
      <div className="lg:col-span-2 rounded-2xl p-5 border" style={{background: CARD, borderColor: 'rgba(248,113,113,0.2)'}}>
        <h3 className="text-sm font-semibold mb-2" style={{color: ERROR}}>Danger zone</h3>
        <p className="text-xs mb-3" style={{color: MUTED}}>Manage your organizations. Deleting an organization is permanent.</p>
        <div className="flex gap-2 flex-wrap">
          {/* Note: real delete-org button is in the sidebar's org list — this is a placeholder */}
          <div className="text-xs" style={{color: DIM}}>Use the sidebar to switch or delete organizations.</div>
        </div>
      </div>
    </div>
  </div>;
}

// ═══════════════════════════════════════════════════════════════
// Modals
// ═══════════════════════════════════════════════════════════════

function CreateOrgModal({ onClose, onSubmit }: { onClose: () => void; onSubmit: (d: { name: string; description: string }) => Promise<boolean> }) {
  const [loading, setLoading] = useState(false);
  const submit = async (e: React.FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    const f = new FormData(e.currentTarget);
    setLoading(true);
    const ok = await onSubmit({ name: f.get('name') as string, description: f.get('desc') as string });
    setLoading(false);
    if (ok) onClose();
  };
  return <Modal onClose={onClose} title="Create organization" subtitle="A workspace for your agents and tasks">
    <form onSubmit={submit}>
      <Field name="name" label="Name" placeholder="e.g. Acme Corp" autoFocus/>
      <Field name="desc" label="Description (optional)" placeholder="What does this organization do?"/>
      <Submit label="Create organization" loading={loading}/>
    </form>
  </Modal>;
}

function CreateAgentModal({ org, onClose, onSubmit }: { org: T; onClose: () => void; onSubmit: (d: { name: string; role: string; description: string }) => Promise<boolean> }) {
  const [loading, setLoading] = useState(false);
  const [role, setRole] = useState('');
  const rolePresets: Record<string, string> = {
    Developer: 'Writes code, builds features, fixes bugs',
    Reviewer: 'Reviews code for quality, safety, best practices',
    Tester: 'Runs tests, finds edge cases, validates behavior',
    DevOps: 'Manages CI/CD, deploys, monitors infrastructure',
    Analyst: 'Analyzes data, generates reports, finds insights',
  };
  const submit = async (e: React.FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    const f = new FormData(e.currentTarget);
    setLoading(true);
    const ok = await onSubmit({ name: f.get('name') as string, role: f.get('role') as string, description: f.get('desc') as string });
    setLoading(false);
    if (ok) onClose();
  };
  return <Modal onClose={onClose} title="Create agent" subtitle={`Will be added to ${org.name}`}>
    <form onSubmit={submit}>
      <Field name="name" label="Name" placeholder="e.g. DeployBot" autoFocus/>
      <div className="mb-3">
        <label className="block text-[10px] mb-1.5 font-medium uppercase tracking-wider" style={{color: MUTED}}>Role</label>
        <select name="role" value={role} onChange={e => setRole(e.target.value)} required className="w-full px-4 py-3 rounded-xl text-sm outline-none" style={{background: INPUT, border: '1px solid rgba(99,102,241,0.15)', color: TEXT}}>
          <option value="" disabled>Select a role…</option>
          {Object.keys(rolePresets).map(r => <option key={r} value={r}>{r}</option>)}
        </select>
        {role && <p className="text-[11px] mt-1.5" style={{color: MUTED}}>{rolePresets[role]}</p>}
      </div>
      <Field name="desc" label="Description (optional)" placeholder="What does this agent specialize in?"/>
      <Submit label="Create agent" loading={loading}/>
    </form>
  </Modal>;
}

function AssignTaskModal({ org, agent, onClose, onSubmit }: { org: T; agent: T; onClose: () => void; onSubmit: (d: { description: string }) => Promise<boolean> }) {
  const [loading, setLoading] = useState(false);
  const submit = async (e: React.FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    const f = new FormData(e.currentTarget);
    const desc = (f.get('task') as string).trim();
    if (!desc) return;
    setLoading(true);
    const ok = await onSubmit({ description: desc });
    setLoading(false);
    if (ok) onClose();
  };
  return <Modal onClose={onClose} title={`Assign task to ${agent.name}`} subtitle={`${agent.role} · ${org.name}`}>
    <form onSubmit={submit}>
      <div className="mb-3">
        <label className="block text-[10px] mb-1.5 font-medium uppercase tracking-wider" style={{color: MUTED}}>Task description</label>
        <textarea name="task" required autoFocus rows={5} placeholder="What should this agent do? Be specific about the goal, inputs, and expected output." className="w-full px-4 py-3 rounded-xl text-sm outline-none resize-none" style={{background: INPUT, border: '1px solid rgba(99,102,241,0.15)', color: TEXT}}/>
        <div className="flex gap-1.5 mt-2 flex-wrap">
          {['Analyze codebase and suggest improvements', 'Write unit tests for the auth module', 'Deploy latest build to staging', 'Generate weekly performance report'].map(s => <Suggestion key={s} text={s}/>)}
        </div>
      </div>
      <Submit label="Assign task" loading={loading}/>
    </form>
  </Modal>;
}

function Suggestion({ text }: { text: string }) {
  return <button type="button" onClick={e => { const ta = (e.currentTarget.closest('form')?.querySelector('textarea[name=task]') as HTMLTextAreaElement); if (ta) { ta.value = text; ta.dispatchEvent(new Event('input', { bubbles: true })); ta.focus(); } }} className="px-2.5 py-1 rounded-lg text-[10px] transition-colors" style={{background: 'rgba(99,102,241,0.1)', color: IDLE, border: '1px solid rgba(99,102,241,0.2)'}}>
    <I.Sparkle className="w-2.5 h-2.5 inline mr-1"/>{text.length > 30 ? text.slice(0, 30) + '…' : text}
  </button>;
}

function OrgSwitcher({ orgs, current, onSelect, onCreate, onClose }: { orgs: T[]; current: T | null; onSelect: (o: T) => void; onCreate: () => void; onClose: () => void }) {
  return <Modal onClose={onClose} title="Switch organization" size="sm">
    <div className="space-y-1 max-h-80 overflow-y-auto -mx-2 px-2">
      {orgs.map(o => (
        <button key={o.id} onClick={() => onSelect(o)} className="w-full flex items-center gap-3 p-3 rounded-xl transition-colors text-left" style={current?.id === o.id ? {background: 'rgba(99,102,241,0.15)'} : {}}>
          <I.Org className="w-4 h-4 shrink-0" style={{color: current?.id === o.id ? IDLE : MUTED}}/>
          <div className="flex-1 min-w-0">
            <div className="text-sm font-medium truncate">{o.name}</div>
            {o.description && <div className="text-[11px] truncate" style={{color: MUTED}}>{o.description}</div>}
          </div>
          {current?.id === o.id && <I.Check className="w-4 h-4" style={{color: SUCCESS}}/>}
        </button>
      ))}
    </div>
    <button onClick={onCreate} className="w-full mt-3 py-2.5 border border-dashed rounded-xl text-sm flex items-center justify-center gap-2 transition-colors" style={{borderColor: BORDER, color: MUTED}}>
      <I.Plus className="w-3.5 h-3.5"/>New organization
    </button>
  </Modal>;
}
