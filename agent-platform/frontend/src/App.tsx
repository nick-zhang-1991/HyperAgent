import './index.css';
import AuthPage from './AuthPage';
import { useState } from 'react';

export default function App() {
  const [token, setToken] = useState(localStorage.getItem('token') || '');

  const onAuth = (t: string, u: any) => {
    setToken(t);
    localStorage.setItem('token', t);
  };

  const logout = () => { setToken(''); localStorage.removeItem('token'); };

  if (!token) return <AuthPage onAuth={onAuth} />;

  return (
    <div style={{minHeight:'100vh', background:'#0a0a0f', color:'white', display:'flex', flexDirection:'column', alignItems:'center', justifyContent:'center', padding:'2rem'}}>
      <div style={{textAlign:'center', marginBottom:'2rem'}}>
        <h1 style={{fontSize:'1.5rem', fontWeight:'bold', marginBottom:'0.5rem'}}>Agent Platform</h1>
        <p style={{color:'#6b7280', fontSize:'0.875rem'}}>Dashboard — Backend v3 with Auth</p>
        <button onClick={logout} style={{marginTop:'1rem', padding:'0.5rem 1rem', background:'#1f2937', borderRadius:'0.5rem', fontSize:'0.875rem', color:'#9ca3af', border:'none', cursor:'pointer'}}>Sign Out</button>
      </div>
      <div style={{background:'#0d0d15', border:'1px solid #1f2937', borderRadius:'0.75rem', padding:'2rem', maxWidth:'28rem', width:'100%', textAlign:'center'}}>
        <div style={{fontSize:'3rem', marginBottom:'1rem'}}>🚀</div>
        <h2 style={{fontSize:'1.125rem', fontWeight:600, marginBottom:'0.5rem'}}>Full Dashboard</h2>
        <p style={{fontSize:'0.875rem', color:'#6b7280'}}>Org + Agent + Task management available.</p>
        <p style={{fontSize:'0.75rem', color:'#4b5563', marginTop:'1rem'}}>API: http://127.0.0.1:4000</p>
      </div>
    </div>
  );
}
