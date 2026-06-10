import { useState } from 'react';
import { tt, sl, al, gl } from './i18n';

export default function AuthPage({ onAuth }: { onAuth: (t: string, u: any) => void }) {
  const [mode, setMode] = useState<'login'|'register'>('login');
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [name, setName] = useState('');
  const [error, setError] = useState('');

  const submit = async () => {
    setError('');
    const api = '';
    const ep = mode === 'login' ? '/api/auth/login' : '/api/auth/register';
    const body: any = { email, password };
    if (mode === 'register') body.name = name || email.split('@')[0];
    try {
      const r = await fetch(api + ep, { method:'POST', headers:{'Content-Type':'application/json'}, body:JSON.stringify(body) });
      const d = await r.json();
      if (r.ok) { onAuth(d.token, d.user) }
      else { setError(d.error || 'Failed') }
    } catch { setError('Connection failed') }
  };

  return (
    <div className="min-h-screen flex items-center justify-center p-4 relative overflow-hidden" style={{background:'linear-gradient(135deg, #05051a 0%, #0a0a2e 30%, #0d0d28 60%, #060622 100%)'}}>
      {/* Animated grid background */}
      <div className="absolute inset-0 opacity-[0.03]" style={{backgroundImage:'linear-gradient(rgba(99,102,241,0.3) 1px, transparent 1px), linear-gradient(90deg, rgba(99,102,241,0.3) 1px, transparent 1px)', backgroundSize:'60px 60px'}} />
      
      {/* Glow orbs */}
      <div className="absolute top-1/4 -left-32 w-96 h-96 rounded-full blur-[128px] opacity-20" style={{background:'radial-gradient(circle, #6366f1, transparent)'}} />
      <div className="absolute bottom-1/4 -right-32 w-96 h-96 rounded-full blur-[128px] opacity-15" style={{background:'radial-gradient(circle, #06b6d4, transparent)'}} />

      <div className="w-full max-w-sm relative z-10">
        {/* Brand */}
        <div className="text-center mb-8">
          <div className="w-20 h-20 mx-auto mb-4 rounded-2xl flex items-center justify-center relative" style={{background:'linear-gradient(135deg, #6366f1, #8b5cf6, #06b6d4)'}}>
            <svg className="w-10 h-10 text-white" fill="none" stroke="currentColor" viewBox="0 0 24 24"><path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M9.75 3.104v5.714a2.25 2.25 0 01-.659 1.591L5 14.5M9.75 3.104c-.251.023-.501.05-.75.082m.75-.082a24.301 24.301 0 014.5 0m0 0v5.714c0 .597.237 1.17.659 1.591L19.8 15.3M14.25 3.104c.251.023.501.05.75.082M19.8 15.3l-1.57.393A9.065 9.065 0 0112 15a9.065 9.065 0 00-6.23.693L5 14.5m14.8.8l1.402 1.402c1.232 1.232.65 3.318-1.067 3.611A48.309 48.309 0 0112 21c-2.773 0-5.491-.235-8.135-.687-1.718-.293-2.3-2.379-1.067-3.61L5 14.5" /></svg>
          </div>
          <h1 className="text-3xl font-bold tracking-tight" style={{background:'linear-gradient(135deg, #e0e7ff, #a5b4fc, #67e8f9)', WebkitBackgroundClip:'text', WebkitTextFillColor:'transparent'}}>HyperAgent</h1>
          <p className="text-sm mt-2 font-light tracking-wide" style={{color:'rgba(148,163,184,0.8)'}}>The best agent on earth</p>
        </div>

        {/* Language selector - positioned nicely at top of card */}
        <div className="mb-4 flex justify-end">
          <select value={gl()} onChange={e => sl(e.target.value)} className="text-xs py-1.5 px-3 rounded-lg border appearance-none cursor-pointer" style={{background:'rgba(15,15,35,0.8)', borderColor:'rgba(99,102,241,0.2)', color:'rgba(148,163,184,0.9)'}}>
            {al().map(l => <option key={l.code} value={l.code}>{l.name}</option>)}
          </select>
        </div>

        {/* Card */}
        <div className="rounded-2xl p-6 backdrop-blur-xl border" style={{background:'rgba(15,15,40,0.7)', borderColor:'rgba(99,102,241,0.15)'}}>
          {/* Tabs */}
          <div className="flex mb-6 rounded-xl p-1" style={{background:'rgba(10,10,25,0.8)'}}>
            <button onClick={() => setMode('login')} className={`flex-1 py-2.5 text-sm rounded-lg font-medium transition-all duration-300 ${mode==='login' ? 'text-white shadow-lg' : ''}`} style={mode==='login'?{background:'linear-gradient(135deg, #6366f1, #8b5cf6)'}:{color:'rgba(148,163,184,0.6)'}}>{tt('signIn')}</button>
            <button onClick={() => setMode('register')} className={`flex-1 py-2.5 text-sm rounded-lg font-medium transition-all duration-300 ${mode==='register' ? 'text-white shadow-lg' : ''}`} style={mode==='register'?{background:'linear-gradient(135deg, #6366f1, #8b5cf6)'}:{color:'rgba(148,163,184,0.6)'}}>{tt('register')}</button>
          </div>

          {mode === 'register' && (
            <input value={name} onChange={e => setName(e.target.value)} placeholder={tt('name')} className="w-full px-4 py-3 rounded-xl text-sm mb-3 outline-none transition-all duration-300 focus:border-opacity-100" style={{background:'rgba(10,10,25,0.8)', border:'1px solid rgba(99,102,241,0.15)', color:'#e2e8f0'}} />
          )}
          <input value={email} onChange={e => setEmail(e.target.value)} placeholder={tt('email')} type="email" className="w-full px-4 py-3 rounded-xl text-sm mb-3 outline-none transition-all duration-300 focus:border-opacity-100" style={{background:'rgba(10,10,25,0.8)', border:'1px solid rgba(99,102,241,0.15)', color:'#e2e8f0'}} />
          <input value={password} onChange={e => setPassword(e.target.value)} placeholder={tt('password')} type="password" className="w-full px-4 py-3 rounded-xl text-sm mb-4 outline-none transition-all duration-300 focus:border-opacity-100" style={{background:'rgba(10,10,25,0.8)', border:'1px solid rgba(99,102,241,0.15)', color:'#e2e8f0'}} onKeyDown={e => e.key === 'Enter' && submit()} />

          {error && <p className="text-xs mb-3 px-1" style={{color:'#f87171'}}>{error}</p>}

          <button onClick={submit} className="w-full py-3 rounded-xl text-sm font-semibold transition-all duration-300 hover:shadow-xl hover:shadow-indigo-500/25 active:scale-[0.98]" style={{background:'linear-gradient(135deg, #6366f1, #8b5cf6)', color:'white'}}>
            {mode === 'login' ? tt('signIn') : tt('register')}
          </button>
        </div>

        <p className="text-center text-xs mt-6" style={{color:'rgba(148,163,184,0.4)'}}>Multi-Agent Orchestration · 20 Languages · SQLite · JWT</p>
      </div>
    </div>
  );
}
