import { jsx as _jsx, jsxs as _jsxs } from "react/jsx-runtime";
import './index.css';
import { useState, useEffect, useCallback } from 'react';
const API = 'http://127.0.0.1:4000';
const WS = 'ws://127.0.0.1:4000/api/ws';
export default function App() {
    return (_jsxs("div", { className: "h-screen bg-[#0a0a0f] text-gray-100 flex font-sans", children: [_jsx(Sidebar, {}), _jsx(MainPanel, {})] }));
}
// ── Sidebar ──
function Sidebar() {
    const [orgs, setOrgs] = useState([]);
    const [selected, setSelected] = useOrgStore(s => [s.selectedOrg, s.setSelectedOrg]);
    const [showCreate, setShowCreate] = useState(false);
    const load = useCallback(async () => {
        try {
            const r = await fetch(`${API}/api/orgs`);
            if (!r.ok)
                throw new Error(`HTTP ${r.status}`);
            setOrgs(await r.json());
        }
        catch (e) {
            console.warn('Backend not available:', e);
        }
    }, []);
    useEffect(() => { load(); }, [load]);
    // Refresh on WS events
    useEffect(() => {
        const ws = new WebSocket(WS);
        ws.onmessage = () => load();
        return () => ws.close();
    }, [load]);
    return (_jsxs("div", { className: "w-64 bg-[#0d0d15] border-r border-gray-800 flex flex-col", children: [_jsxs("div", { className: "p-5 border-b border-gray-800", children: [_jsxs("div", { className: "flex items-center gap-2 mb-1", children: [_jsx("div", { className: "w-7 h-7 rounded-lg bg-gradient-to-br from-indigo-500 to-purple-600 flex items-center justify-center text-sm font-bold", children: "A" }), _jsx("span", { className: "font-semibold text-base", children: "Agent Platform" })] }), _jsx("p", { className: "text-xs text-gray-500 mt-1", children: "Orchestration Dashboard" })] }), _jsxs("div", { className: "flex-1 overflow-y-auto p-3", children: [_jsxs("div", { className: "flex items-center justify-between mb-3", children: [_jsx("span", { className: "text-xs font-medium text-gray-400 uppercase tracking-wider", children: "Organizations" }), _jsx("button", { onClick: () => setShowCreate(true), className: "w-6 h-6 rounded-md bg-indigo-600 hover:bg-indigo-500 text-white flex items-center justify-center text-lg transition-colors", children: "+" })] }), orgs.map(o => (_jsxs("button", { onClick: () => setSelected(o.id), className: `w-full text-left px-3 py-2.5 rounded-lg mb-1 transition-all duration-150 ${selected === o.id ? 'bg-indigo-600/20 border border-indigo-500/30 text-white' : 'hover:bg-gray-800/50 text-gray-400 border border-transparent'}`, children: [_jsx("div", { className: "text-sm font-medium truncate", children: o.name }), _jsx("div", { className: "text-xs text-gray-500 truncate mt-0.5", children: o.description })] }, o.id))), orgs.length === 0 && _jsx("p", { className: "text-xs text-gray-600 text-center mt-8", children: "No organizations yet" })] }), _jsx(CreateOrgModal, { open: showCreate, onClose: () => setShowCreate(false) })] }));
}
// ── Main Panel ──
function MainPanel() {
    const selected = useOrgStore(s => s.selectedOrg);
    const [orgs, setOrgs] = useState([]);
    useEffect(() => {
        fetch(`${API}/api/orgs`).then(r => r.json()).then(setOrgs);
    }, [selected]);
    const org = orgs.find(o => o.id === selected);
    if (!selected || !org) {
        return (_jsx("div", { className: "flex-1 flex items-center justify-center", children: _jsxs("div", { className: "text-center", children: [_jsx("div", { className: "w-16 h-16 rounded-2xl bg-gradient-to-br from-indigo-500 to-purple-600 mx-auto flex items-center justify-center text-2xl font-bold mb-4", children: "A" }), _jsx("h2", { className: "text-xl font-semibold mb-2", children: "Agent Platform" }), _jsx("p", { className: "text-gray-500 text-sm", children: "Select an organization or create a new one to begin" })] }) }));
    }
    return (_jsxs("div", { className: "flex-1 flex flex-col overflow-hidden", children: [_jsx(Header, { org: org }), _jsx(AgentGrid, { orgId: selected })] }));
}
// ── Header ──
function Header({ org }) {
    const [showAgent, setShowAgent] = useState(false);
    const setSelected = useOrgStore(s => s.setSelectedOrg);
    return (_jsxs("div", { className: "px-6 py-4 bg-[#0d0d15] border-b border-gray-800 flex items-center justify-between", children: [_jsx("div", { children: _jsxs("div", { className: "flex items-center gap-3", children: [_jsx("button", { onClick: () => setSelected(null), className: "text-gray-500 hover:text-gray-300 text-sm", children: "\u2190 Back" }), _jsx("h1", { className: "text-lg font-semibold", children: org.name }), _jsx("span", { className: "px-2 py-0.5 rounded-full text-xs bg-gray-800 text-gray-400", children: org.description })] }) }), _jsxs("button", { onClick: () => setShowAgent(true), className: "px-4 py-2 bg-indigo-600 hover:bg-indigo-500 text-white text-sm rounded-lg font-medium transition-colors flex items-center gap-2", children: [_jsx("span", { children: "+" }), " Create Agent"] }), _jsx(CreateAgentModal, { orgId: org.id, open: showAgent, onClose: () => setShowAgent(false) })] }));
}
// ── Agent Grid ──
function AgentGrid({ orgId }) {
    const [agents, setAgents] = useState([]);
    const [tasks, setTasks] = useState([]);
    const [events, setEvents] = useState([]);
    const [agentProgress, setAgentProgress] = useState({});
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
                setAgentProgress(prev => ({ ...prev, [agentId]: action }));
            }
            else if (msg.startsWith('done:')) {
                setTimeout(() => setAgentProgress(prev => {
                    const next = { ...prev };
                    Object.keys(next).forEach(k => { if (next[k].includes(msg.substring(5, 11)))
                        delete next[k]; });
                    return next;
                }), 3000);
            }
            load();
        };
        return () => ws.close();
    }, [load]);
    const filteredTasks = tasks.filter(t => {
        const matchSearch = !search || t.description.toLowerCase().includes(search.toLowerCase()) || (t.result || '').toLowerCase().includes(search.toLowerCase());
        const matchStatus = !statusFilter || t.status === statusFilter;
        return matchSearch && matchStatus;
    });
    const agentTasks = (agentId) => filteredTasks.filter(t => t.agent_id === agentId).slice(-10);
    return (_jsxs("div", { className: "flex-1 overflow-y-auto p-6", children: [_jsxs("div", { className: "flex items-center gap-3 mb-4", children: [_jsxs("div", { className: "flex-1 relative", children: [_jsx("svg", { className: "absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-gray-600", fill: "none", stroke: "currentColor", viewBox: "0 0 24 24", children: _jsx("path", { strokeLinecap: "round", strokeLinejoin: "round", strokeWidth: 2, d: "M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" }) }), _jsx("input", { type: "text", placeholder: "Search tasks...", value: search, onChange: e => setSearch(e.target.value), className: "w-full pl-9 pr-4 py-2 rounded-lg bg-[#0d0d15] border border-gray-800 text-sm text-gray-300 placeholder-gray-600 focus:outline-none focus:border-indigo-500 transition-colors" })] }), _jsxs("select", { value: statusFilter, onChange: e => setStatusFilter(e.target.value), className: "px-3 py-2 rounded-lg bg-[#0d0d15] border border-gray-800 text-sm text-gray-400 focus:outline-none focus:border-indigo-500", children: [_jsx("option", { value: "", children: "All Status" }), _jsx("option", { value: "completed", children: "Completed" }), _jsx("option", { value: "running", children: "Running" }), _jsx("option", { value: "pending", children: "Pending" }), _jsx("option", { value: "failed", children: "Failed" })] })] }), _jsxs("div", { className: "grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4 mb-6", children: [agents.map(a => (_jsx(AgentCard, { agent: a, tasks: agentTasks(a.id), orgId: orgId, onUpdate: load, progress: agentProgress }, a.id))), agents.length === 0 && (_jsxs("div", { className: "col-span-full text-center py-20 text-gray-600", children: [_jsx("div", { className: "text-4xl mb-3", children: "\uD83E\uDD16" }), _jsx("p", { className: "text-sm", children: "No agents yet. Create your first agent to get started." })] }))] }), _jsxs("div", { className: "rounded-xl border border-gray-800 bg-[#0d0d15] overflow-hidden", children: [_jsxs("div", { className: "px-4 py-2.5 border-b border-gray-800 flex items-center gap-2", children: [_jsx("div", { className: "w-2 h-2 rounded-full bg-green-500 animate-pulse" }), _jsx("span", { className: "text-xs font-medium text-gray-400", children: "LIVE EVENTS" }), _jsxs("span", { className: "text-xs text-gray-600 ml-auto", children: [events.length, " events"] })] }), _jsxs("div", { className: "p-3 max-h-48 overflow-y-auto font-mono text-xs text-gray-500 space-y-1", children: [events.slice(-30).map((e, i) => (_jsx("div", { className: "hover:text-gray-300 transition-colors", children: e }, i))), events.length === 0 && _jsx("div", { className: "text-gray-700", children: "Waiting for events..." })] })] })] }));
}
// ── Agent Card ──
function AgentCard({ agent, tasks, orgId, onUpdate, progress }) {
    const [showTask, setShowTask] = useState(false);
    const pulse = agent.status === 'working';
    const statusColor = {
        idle: 'bg-green-500', working: 'bg-amber-500', completed: 'bg-blue-500', error: 'bg-red-500'
    };
    const taskColor = {
        pending: 'text-gray-500', running: 'text-amber-400', completed: 'text-green-400', failed: 'text-red-400'
    };
    return (_jsxs("div", { className: `rounded-xl border bg-[#0d0d15] overflow-hidden transition-all duration-300 ${pulse ? 'border-amber-500/30 shadow-[0_0_20px_rgba(245,158,11,0.1)]' : 'border-gray-800 hover:border-gray-700'}`, children: [_jsxs("div", { className: "p-4 border-b border-gray-800", children: [_jsxs("div", { className: "flex items-start justify-between mb-2", children: [_jsxs("div", { children: [_jsx("h3", { className: "font-semibold text-sm", children: agent.name }), _jsx("p", { className: "text-xs text-gray-500 mt-0.5", children: agent.role })] }), _jsxs("span", { className: `inline-flex items-center gap-1.5 px-2 py-0.5 rounded-full text-xs font-medium ${agent.status === 'idle' ? 'bg-green-500/10 text-green-400' :
                                    agent.status === 'working' ? 'bg-amber-500/10 text-amber-400' :
                                        agent.status === 'error' ? 'bg-red-500/10 text-red-400' :
                                            'bg-blue-500/10 text-blue-400'}`, children: [_jsx("span", { className: `w-1.5 h-1.5 rounded-full ${statusColor[agent.status]} ${pulse ? 'animate-pulse' : ''}` }), agent.status] })] }), _jsx("p", { className: "text-xs text-gray-600 line-clamp-2", children: agent.description }), _jsxs("div", { className: "flex gap-4 mt-3 pt-2 border-t border-gray-800", children: [_jsxs("div", { className: "text-center", children: [_jsx("div", { className: "text-xs font-semibold text-gray-300", children: agent.total_tasks }), _jsx("div", { className: "text-[10px] text-gray-600", children: "Total" })] }), _jsxs("div", { className: "text-center", children: [_jsx("div", { className: "text-xs font-semibold text-green-400", children: agent.completed_tasks }), _jsx("div", { className: "text-[10px] text-gray-600", children: "Done" })] }), _jsxs("div", { className: "text-center", children: [_jsxs("div", { className: "text-xs font-semibold text-indigo-400", children: [agent.total_tasks > 0 ? Math.round(agent.completed_tasks / agent.total_tasks * 100) : 0, "%"] }), _jsx("div", { className: "text-[10px] text-gray-600", children: "Rate" })] })] })] }), _jsxs("div", { className: "px-4 py-3 space-y-2 min-h-[80px]", children: [tasks.map(t => (_jsxs("div", { className: "flex items-start gap-2 text-xs", children: [_jsx("span", { className: taskColor[t.status], children: t.status === 'completed' ? '✅' : t.status === 'running' ? '🔄' : t.status === 'failed' ? '❌' : '⏳' }), _jsxs("div", { className: "flex-1 min-w-0", children: [_jsx("span", { className: "truncate block", children: t.description }), t.result && _jsx("span", { className: "text-green-500/70", children: t.result.slice(0, 60) })] })] }, t.id))), tasks.length === 0 && !progress[agent.id] && (_jsx("p", { className: "text-xs text-gray-700 text-center py-3", children: "No tasks assigned" }))] }), _jsx("div", { className: "p-3 border-t border-gray-800", children: _jsx("button", { onClick: () => setShowTask(true), disabled: agent.status === 'working', className: `w-full py-2 rounded-lg text-xs font-medium transition-all duration-200 ${agent.status === 'working'
                        ? 'bg-gray-800 text-gray-600 cursor-not-allowed'
                        : 'bg-indigo-600/10 text-indigo-400 hover:bg-indigo-600/20 border border-indigo-500/20'}`, children: agent.status === 'working' ? '⏳ Processing...' : '📝 Assign New Task' }) }), _jsx(AssignTaskModal, { agent: agent, orgId: orgId, open: showTask, onClose: () => { setShowTask(false); onUpdate(); } })] }));
}
// ── Modals ──
function Modal({ open, onClose, title, children }) {
    if (!open)
        return null;
    return (_jsxs("div", { className: "fixed inset-0 z-50 flex items-center justify-center", children: [_jsx("div", { className: "absolute inset-0 bg-black/60 backdrop-blur-sm", onClick: onClose }), _jsxs("div", { className: "relative bg-[#0d0d15] border border-gray-800 rounded-2xl w-full max-w-md mx-4 shadow-2xl", children: [_jsxs("div", { className: "flex items-center justify-between p-5 border-b border-gray-800", children: [_jsx("h2", { className: "text-base font-semibold", children: title }), _jsx("button", { onClick: onClose, className: "w-7 h-7 rounded-lg hover:bg-gray-800 flex items-center justify-center text-gray-500 hover:text-gray-300 transition-colors", children: "\u2715" })] }), _jsx("div", { className: "p-5", children: children })] })] }));
}
function CreateOrgModal({ open, onClose }) {
    const [name, setName] = useState('');
    const [desc, setDesc] = useState('');
    const [loading, setLoading] = useState(false);
    const setSelected = useOrgStore(s => s.setSelectedOrg);
    const submit = async () => {
        if (!name.trim())
            return;
        setLoading(true);
        const r = await fetch(`${API}/api/orgs`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ name, description: desc }) });
        const org = await r.json();
        setSelected(org.id);
        setLoading(false);
        onClose();
    };
    return (_jsx(Modal, { open: open, onClose: onClose, title: "Create Organization", children: _jsxs("div", { className: "space-y-4", children: [_jsxs("div", { children: [_jsx("label", { className: "block text-xs font-medium text-gray-400 mb-1.5", children: "Name" }), _jsx("input", { value: name, onChange: e => setName(e.target.value), placeholder: "Engineering Team", autoFocus: true, className: "w-full px-3 py-2 rounded-lg bg-gray-900 border border-gray-700 text-sm text-gray-200 placeholder-gray-600 focus:outline-none focus:border-indigo-500 transition-colors" })] }), _jsxs("div", { children: [_jsx("label", { className: "block text-xs font-medium text-gray-400 mb-1.5", children: "Description" }), _jsx("textarea", { value: desc, onChange: e => setDesc(e.target.value), placeholder: "What does this team do?", rows: 2, className: "w-full px-3 py-2 rounded-lg bg-gray-900 border border-gray-700 text-sm text-gray-200 placeholder-gray-600 focus:outline-none focus:border-indigo-500 transition-colors resize-none" })] }), _jsxs("div", { className: "flex gap-3 pt-2", children: [_jsx("button", { onClick: onClose, className: "flex-1 py-2 rounded-lg bg-gray-800 text-gray-400 text-sm font-medium hover:bg-gray-700 transition-colors", children: "Cancel" }), _jsx("button", { onClick: submit, disabled: loading || !name.trim(), className: "flex-1 py-2 rounded-lg bg-indigo-600 text-white text-sm font-medium hover:bg-indigo-500 disabled:opacity-50 transition-colors", children: loading ? 'Creating...' : 'Create' })] })] }) }));
}
function CreateAgentModal({ orgId, open, onClose }) {
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
    const applyTemplate = (t) => {
        setRole(t.role);
        setDesc(t.desc);
    };
    const submit = async () => {
        if (!name.trim() || !role.trim())
            return;
        setLoading(true);
        await fetch(`${API}/api/orgs/${orgId}/agents`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ name, role, description: desc }) });
        setLoading(false);
        onClose();
    };
    return (_jsx(Modal, { open: open, onClose: onClose, title: "Create Agent", children: _jsxs("div", { className: "space-y-4", children: [_jsxs("div", { children: [_jsx("label", { className: "block text-xs font-medium text-gray-400 mb-1.5", children: "Agent Name" }), _jsx("input", { value: name, onChange: e => setName(e.target.value), placeholder: "e.g. SecurityBot", autoFocus: true, className: "w-full px-3 py-2 rounded-lg bg-gray-900 border border-gray-700 text-sm text-gray-200 placeholder-gray-600 focus:outline-none focus:border-indigo-500 transition-colors" })] }), _jsxs("div", { children: [_jsx("label", { className: "block text-xs font-medium text-gray-400 mb-1.5", children: "Role" }), _jsx("input", { value: role, onChange: e => setRole(e.target.value), placeholder: "e.g. Code Reviewer", className: "w-full px-3 py-2 rounded-lg bg-gray-900 border border-gray-700 text-sm text-gray-200 placeholder-gray-600 focus:outline-none focus:border-indigo-500 transition-colors" })] }), _jsxs("div", { children: [_jsx("label", { className: "block text-xs font-medium text-gray-400 mb-1.5", children: "Role Templates" }), _jsx("div", { className: "flex flex-wrap gap-1.5", children: ROLE_TEMPLATES.map(t => (_jsx("button", { onClick: () => applyTemplate(t), className: "px-2.5 py-1 rounded-md text-xs bg-gray-800 border border-gray-700 text-gray-400 hover:border-indigo-500 hover:text-indigo-400 transition-colors", children: t.role }, t.role))) })] }), _jsxs("div", { children: [_jsx("label", { className: "block text-xs font-medium text-gray-400 mb-1.5", children: "Role Description" }), _jsx("textarea", { value: desc, onChange: e => setDesc(e.target.value), placeholder: "Describe what this agent does in natural language...", rows: 3, className: "w-full px-3 py-2 rounded-lg bg-gray-900 border border-gray-700 text-sm text-gray-200 placeholder-gray-600 focus:outline-none focus:border-indigo-500 transition-colors resize-none" })] }), _jsxs("div", { className: "flex gap-3 pt-2", children: [_jsx("button", { onClick: onClose, className: "flex-1 py-2 rounded-lg bg-gray-800 text-gray-400 text-sm font-medium hover:bg-gray-700 transition-colors", children: "Cancel" }), _jsx("button", { onClick: submit, disabled: loading || !name.trim() || !role.trim(), className: "flex-1 py-2 rounded-lg bg-indigo-600 text-white text-sm font-medium hover:bg-indigo-500 disabled:opacity-50 transition-colors", children: loading ? 'Creating...' : 'Create Agent' })] })] }) }));
}
function AssignTaskModal({ agent, orgId, open, onClose }) {
    const [desc, setDesc] = useState('');
    const [loading, setLoading] = useState(false);
    const submit = async () => {
        if (!desc.trim())
            return;
        setLoading(true);
        await fetch(`${API}/api/orgs/${orgId}/agents/${agent.id}/tasks`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ description: desc }) });
        setLoading(false);
        setDesc('');
        onClose();
    };
    return (_jsx(Modal, { open: open, onClose: onClose, title: `Assign Task → ${agent.name}`, children: _jsxs("div", { className: "space-y-4", children: [_jsxs("div", { className: "text-xs text-gray-500", children: ["Agent role: ", _jsx("span", { className: "text-indigo-400", children: agent.role }), " \u2014 ", agent.description] }), _jsxs("div", { children: [_jsx("label", { className: "block text-xs font-medium text-gray-400 mb-1.5", children: "Task Description" }), _jsx("p", { className: "text-xs text-gray-600 mb-2", children: "Describe the task in natural language. The agent will use its role definition to understand and execute it." }), _jsx("textarea", { value: desc, onChange: e => setDesc(e.target.value), placeholder: "e.g. Review all unwrap() usage in src/ and suggest safer alternatives using ? or match", rows: 4, autoFocus: true, className: "w-full px-3 py-2 rounded-lg bg-gray-900 border border-gray-700 text-sm text-gray-200 placeholder-gray-600 focus:outline-none focus:border-indigo-500 transition-colors resize-none" })] }), _jsxs("div", { className: "flex gap-3 pt-2", children: [_jsx("button", { onClick: onClose, className: "flex-1 py-2 rounded-lg bg-gray-800 text-gray-400 text-sm font-medium hover:bg-gray-700 transition-colors", children: "Cancel" }), _jsx("button", { onClick: submit, disabled: loading || !desc.trim(), className: "flex-1 py-2 rounded-lg bg-indigo-600 text-white text-sm font-medium hover:bg-indigo-500 disabled:opacity-50 transition-colors", children: loading ? 'Assigning...' : 'Assign Task' })] })] }) }));
}
// ── Zustand-style store (lightweight) ──
function createStore(initial) {
    let state = initial;
    const listeners = new Set();
    return {
        get: () => state,
        set: (fn) => { state = fn(state); listeners.forEach(l => l()); },
        sub: (l) => { listeners.add(l); return () => { listeners.delete(l); }; },
    };
}
const orgStore = createStore({
    selectedOrg: null,
    setSelectedOrg: () => { },
});
function useOrgStore(selector) {
    const [, force] = useState({});
    useEffect(() => { const unsub = orgStore.sub(() => force({})); return unsub; }, []);
    return selector(orgStore.get());
}
// Initialize store
orgStore.set(s => ({ ...s, setSelectedOrg: (id) => orgStore.set(s => ({ ...s, selectedOrg: id })) }));
