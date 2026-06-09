import { jsx as _jsx, jsxs as _jsxs } from "react/jsx-runtime";
import { useState } from 'react';
import { tt, sl, al, gl } from './i18n';
// Auto-detect: use CN2 if accessing from remote, localhost if local
const isRemote = window.location.hostname !== 'localhost' && window.location.hostname !== '127.0.0.1';
const API = isRemote ? `http://${window.location.hostname}:4000` : 'http://127.0.0.1:4000';
export default function AuthPage({ onAuth }) {
    const [mode, setMode] = useState('login');
    const [email, setEmail] = useState('');
    const [password, setPassword] = useState('');
    const [name, setName] = useState('');
    const [error, setError] = useState('');
    const submit = async () => {
        setError('');
        const endpoint = mode === 'login' ? '/api/auth/login' : '/api/auth/register';
        const body = { email, password };
        if (mode === 'register')
            body.name = name || email.split('@')[0];
        try {
            const r = await fetch(API + endpoint, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
            const d = await r.json();
            if (r.ok) {
                onAuth(d.token, d.user);
            }
            else {
                setError(d.error || 'Authentication failed');
            }
        }
        catch (e) {
            setError('Backend not available. Start: agent-platform/backend/cargo run');
        }
    };
    return (_jsx("div", { className: "min-h-screen bg-[#0a0a0f] flex items-center justify-center p-4", children: _jsxs("div", { className: "w-full max-w-sm", children: [_jsxs("div", { className: "text-center mb-8", children: [_jsx("div", { className: "w-16 h-16 rounded-2xl bg-gradient-to-br from-indigo-500 to-purple-600 mx-auto flex items-center justify-center text-2xl font-bold mb-3", children: "A" }), _jsx("h1", { className: "text-xl font-semibold", children: tt('dashboard') }), _jsx("p", { className: "text-sm text-gray-500 mt-1", children: "Multi-Agent Orchestration" }), _jsx("select", { value: gl(), onChange: e => sl(e.target.value), className: "mt-3 text-xs bg-[#06060a] border border-gray-800 rounded px-2 py-1 text-gray-400", children: al().map(l => _jsx("option", { value: l.code, children: l.name }, l.code)) })] }), _jsxs("div", { className: "bg-[#0d0d15] border border-gray-800 rounded-xl p-6", children: [_jsxs("div", { className: "flex mb-6 bg-[#06060a] rounded-lg p-1", children: [_jsx("button", { onClick: () => setMode('login'), className: `flex-1 py-2 text-sm rounded-md transition-colors ${mode === 'login' ? 'bg-indigo-600 text-white' : 'text-gray-500'}`, children: tt('signIn') }), _jsx("button", { onClick: () => setMode('register'), className: `flex-1 py-2 text-sm rounded-md transition-colors ${mode === 'register' ? 'bg-indigo-600 text-white' : 'text-gray-500'}`, children: tt('register') })] }), mode === 'register' && (_jsx("input", { value: name, onChange: e => setName(e.target.value), placeholder: "Name", className: "w-full px-3 py-2.5 rounded-lg bg-[#06060a] border border-gray-800 text-sm mb-3 focus:outline-none focus:border-indigo-500", autoFocus: true })), _jsx("input", { value: email, onChange: e => setEmail(e.target.value), placeholder: "Email", type: "email", className: "w-full px-3 py-2.5 rounded-lg bg-[#06060a] border border-gray-800 text-sm mb-3 focus:outline-none focus:border-indigo-500", autoFocus: mode === 'login' }), _jsx("input", { value: password, onChange: e => setPassword(e.target.value), placeholder: "Password", type: "password", className: "w-full px-3 py-2.5 rounded-lg bg-[#06060a] border border-gray-800 text-sm mb-4 focus:outline-none focus:border-indigo-500", onKeyDown: e => e.key === 'Enter' && submit() }), error && _jsx("p", { className: "text-xs text-red-400 mb-3", children: error }), _jsx("button", { onClick: submit, className: "w-full py-2.5 bg-indigo-600 hover:bg-indigo-500 rounded-lg text-sm font-medium transition-colors", children: mode === 'login' ? 'Sign In' : 'Create Account' })] })] }) }));
}
