import { jsx as _jsx, jsxs as _jsxs } from "react/jsx-runtime";
import { useState } from 'react';
import { tt, sl, al, gl } from './i18n';
import { apiBase } from './api';
export default function AuthPage({ onAuth }) {
    const [mode, setMode] = useState('login');
    const [email, setEmail] = useState('');
    const [password, setPassword] = useState('');
    const [name, setName] = useState('');
    const [error, setError] = useState('');
    const submit = async () => {
        setError('');
        const ep = mode === 'login' ? '/api/auth/login' : '/api/auth/register';
        const body = { email, password };
        if (mode === 'register')
            body.name = name || email.split('@')[0];
        try {
            const r = await fetch(apiBase() + ep, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
            let d = {};
            try {
                d = await r.json();
            }
            catch { }
            if (r.ok) {
                onAuth(d.token, d.user);
            }
            else {
                setError(d.error || `Server error (${r.status})`);
            }
        }
        catch (e) {
            const api = apiBase();
            setError(`Connection failed — backend not reachable at ${api || 'same origin'}`);
        }
    };
    return (_jsxs("div", { className: "min-h-screen flex items-center justify-center p-4 relative overflow-hidden", style: { background: 'linear-gradient(135deg, #05051a 0%, #0a0a2e 30%, #0d0d28 60%, #060622 100%)' }, children: [_jsx("div", { className: "absolute inset-0 opacity-[0.03]", style: { backgroundImage: 'linear-gradient(rgba(99,102,241,0.3) 1px, transparent 1px), linear-gradient(90deg, rgba(99,102,241,0.3) 1px, transparent 1px)', backgroundSize: '60px 60px' } }), _jsx("div", { className: "absolute top-1/4 -left-32 w-96 h-96 rounded-full blur-[128px] opacity-20", style: { background: 'radial-gradient(circle, #6366f1, transparent)' } }), _jsx("div", { className: "absolute bottom-1/4 -right-32 w-96 h-96 rounded-full blur-[128px] opacity-15", style: { background: 'radial-gradient(circle, #06b6d4, transparent)' } }), _jsxs("div", { className: "w-full max-w-sm relative z-10", children: [_jsxs("div", { className: "text-center mb-8", children: [_jsx("div", { className: "w-20 h-20 mx-auto mb-4 rounded-2xl flex items-center justify-center relative", style: { background: 'linear-gradient(135deg, #6366f1, #8b5cf6, #06b6d4)' }, children: _jsx("svg", { className: "w-10 h-10 text-white", fill: "none", stroke: "currentColor", viewBox: "0 0 24 24", children: _jsx("path", { strokeLinecap: "round", strokeLinejoin: "round", strokeWidth: 1.5, d: "M9.75 3.104v5.714a2.25 2.25 0 01-.659 1.591L5 14.5M9.75 3.104c-.251.023-.501.05-.75.082m.75-.082a24.301 24.301 0 014.5 0m0 0v5.714c0 .597.237 1.17.659 1.591L19.8 15.3M14.25 3.104c.251.023.501.05.75.082M19.8 15.3l-1.57.393A9.065 9.065 0 0112 15a9.065 9.065 0 00-6.23.693L5 14.5m14.8.8l1.402 1.402c1.232 1.232.65 3.318-1.067 3.611A48.309 48.309 0 0112 21c-2.773 0-5.491-.235-8.135-.687-1.718-.293-2.3-2.379-1.067-3.61L5 14.5" }) }) }), _jsx("h1", { className: "text-3xl font-bold tracking-tight", style: { background: 'linear-gradient(135deg, #e0e7ff, #a5b4fc, #67e8f9)', WebkitBackgroundClip: 'text', WebkitTextFillColor: 'transparent' }, children: "HyperAgent" }), _jsx("p", { className: "text-sm mt-2 font-light tracking-wide", style: { color: 'rgba(148,163,184,0.8)' }, children: "The best agent on earth" })] }), _jsx("div", { className: "mb-4 flex justify-end", children: _jsx("select", { value: gl(), onChange: e => sl(e.target.value), className: "text-xs py-1.5 px-3 rounded-lg border appearance-none cursor-pointer", style: { background: 'rgba(15,15,35,0.8)', borderColor: 'rgba(99,102,241,0.2)', color: 'rgba(148,163,184,0.9)' }, children: al().map(l => _jsx("option", { value: l.code, children: l.name }, l.code)) }) }), _jsxs("div", { className: "rounded-2xl p-6 backdrop-blur-xl border", style: { background: 'rgba(15,15,40,0.7)', borderColor: 'rgba(99,102,241,0.15)' }, children: [_jsxs("div", { className: "flex mb-6 rounded-xl p-1", style: { background: 'rgba(10,10,25,0.8)' }, children: [_jsx("button", { onClick: () => setMode('login'), className: `flex-1 py-2.5 text-sm rounded-lg font-medium transition-all duration-300 ${mode === 'login' ? 'text-white shadow-lg' : ''}`, style: mode === 'login' ? { background: 'linear-gradient(135deg, #6366f1, #8b5cf6)' } : { color: 'rgba(148,163,184,0.6)' }, children: tt('signIn') }), _jsx("button", { onClick: () => setMode('register'), className: `flex-1 py-2.5 text-sm rounded-lg font-medium transition-all duration-300 ${mode === 'register' ? 'text-white shadow-lg' : ''}`, style: mode === 'register' ? { background: 'linear-gradient(135deg, #6366f1, #8b5cf6)' } : { color: 'rgba(148,163,184,0.6)' }, children: tt('register') })] }), mode === 'register' && (_jsx("input", { value: name, onChange: e => setName(e.target.value), placeholder: tt('name'), className: "w-full px-4 py-3 rounded-xl text-sm mb-3 outline-none transition-all duration-300 focus:border-opacity-100", style: { background: 'rgba(10,10,25,0.8)', border: '1px solid rgba(99,102,241,0.15)', color: '#e2e8f0' } })), _jsx("input", { value: email, onChange: e => setEmail(e.target.value), placeholder: tt('email'), type: "email", className: "w-full px-4 py-3 rounded-xl text-sm mb-3 outline-none transition-all duration-300 focus:border-opacity-100", style: { background: 'rgba(10,10,25,0.8)', border: '1px solid rgba(99,102,241,0.15)', color: '#e2e8f0' } }), _jsx("input", { value: password, onChange: e => setPassword(e.target.value), placeholder: tt('password'), type: "password", className: "w-full px-4 py-3 rounded-xl text-sm mb-4 outline-none transition-all duration-300 focus:border-opacity-100", style: { background: 'rgba(10,10,25,0.8)', border: '1px solid rgba(99,102,241,0.15)', color: '#e2e8f0' }, onKeyDown: e => e.key === 'Enter' && submit() }), error && _jsx("p", { className: "text-xs mb-3 px-1", style: { color: '#f87171' }, children: error }), _jsx("button", { onClick: submit, className: "w-full py-3 rounded-xl text-sm font-semibold transition-all duration-300 hover:shadow-xl hover:shadow-indigo-500/25 active:scale-[0.98]", style: { background: 'linear-gradient(135deg, #6366f1, #8b5cf6)', color: 'white' }, children: mode === 'login' ? tt('signIn') : tt('register') })] })] })] }));
}
