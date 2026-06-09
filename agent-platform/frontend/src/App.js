import { jsx as _jsx, jsxs as _jsxs } from "react/jsx-runtime";
import './index.css';
import AuthPage from './AuthPage';
import { useState } from 'react';
export default function App() {
    const [token, setToken] = useState(localStorage.getItem('token') || '');
    const onAuth = (t, u) => {
        setToken(t);
        localStorage.setItem('token', t);
    };
    const logout = () => { setToken(''); localStorage.removeItem('token'); };
    if (!token)
        return _jsx(AuthPage, { onAuth: onAuth });
    return (_jsxs("div", { style: { minHeight: '100vh', background: '#0a0a0f', color: 'white', display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', padding: '2rem' }, children: [_jsxs("div", { style: { textAlign: 'center', marginBottom: '2rem' }, children: [_jsx("h1", { style: { fontSize: '1.5rem', fontWeight: 'bold', marginBottom: '0.5rem' }, children: "Agent Platform" }), _jsx("p", { style: { color: '#6b7280', fontSize: '0.875rem' }, children: "Dashboard \u2014 Backend v3 with Auth" }), _jsx("button", { onClick: logout, style: { marginTop: '1rem', padding: '0.5rem 1rem', background: '#1f2937', borderRadius: '0.5rem', fontSize: '0.875rem', color: '#9ca3af', border: 'none', cursor: 'pointer' }, children: "Sign Out" })] }), _jsxs("div", { style: { background: '#0d0d15', border: '1px solid #1f2937', borderRadius: '0.75rem', padding: '2rem', maxWidth: '28rem', width: '100%', textAlign: 'center' }, children: [_jsx("div", { style: { fontSize: '3rem', marginBottom: '1rem' }, children: "\uD83D\uDE80" }), _jsx("h2", { style: { fontSize: '1.125rem', fontWeight: 600, marginBottom: '0.5rem' }, children: "Full Dashboard" }), _jsx("p", { style: { fontSize: '0.875rem', color: '#6b7280' }, children: "Org + Agent + Task management available." }), _jsx("p", { style: { fontSize: '0.75rem', color: '#4b5563', marginTop: '1rem' }, children: "API: http://127.0.0.1:4000" })] })] }));
}
