
import { useState, useCallback, useEffect, useRef } from 'react';
import { useI18n } from './i18n/useI18n';
import type { Lang } from './i18n/index';

// Simplified App - rebuilt to fix parse errors
const S = {
  Plus: () => <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><line x1="12" y1="5" x2="12" y2="19"/><line x1="5" y1="12" x2="19" y2="12"/></svg>,
  Send: () => <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><line x1="22" y1="2" x2="11" y2="13"/><polygon points="22 2 15 22 11 13 2 9 22 2"/></svg>,
  Sun: () => <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="12" cy="12" r="5"/><line x1="12" y1="1" x2="12" y2="3"/></svg>,
  Moon: () => <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M21 12.79A9 9 0 1111.21 3 7 7 0 0021 12.79z"/></svg>,
  SearchIcon: () => <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="11" cy="11" r="8"/><line x1="21" y1="21" x2="16.65" y2="16.65"/></svg>,
  Trash: () => <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><polyline points="3 6 5 6 21 6"/><path d="M19 6v14a2 2 0 01-2 2H7a2 2 0 01-2-2V6m3 0V4a2 2 0 012-2h4a2 2 0 012 2v2"/></svg>,
  Export: () => <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" y1="15" x2="12" y2="3"/></svg>,
  Gear: () => <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 00.33 1.82l.06.06a2 2 0 010 2.83 2 2 0 01-2.83 0l-.06-.06a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 01-2 2 2 2 0 01-2-2v-.09A1.65 1.65 0 009 19.4a1.65 1.65 0 00-1.82.33l-.06.06a2 2 0 01-2.83 0 2 2 0 010-2.83l.06-.06A1.65 1.65 0 004.68 15a1.65 1.65 0 00-1.51-1H3a2 2 0 01-2-2 2 2 0 012-2h.09A1.65 1.65 0 004.6 9a1.65 1.65 0 00-.33-1.82l-.06-.06a2 2 0 010-2.83 2 2 0 012.83 0l.06.06A1.65 1.65 0 009 4.68a1.65 1.65 0 001-1.51V3a2 2 0 012-2 2 2 0 012 2v.09a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.06-.06a2 2 0 012.83 0 2 2 0 010 2.83l-.06.06A1.65 1.65 0 0019.4 9a1.65 1.65 0 001.51 1H21a2 2 0 012 2 2 2 0 01-2 2h-.09a1.65 1.65 0 00-1.51 1z"/></svg>,
};

const MODELS = ['deepseek-v4-flash','gpt-4o','claude-3.5-sonnet','gemini-2.0-flash','llama3.1-70b','mixtral-8x7b'];
const fmtTime = (ts:number) => new Date(ts).toLocaleTimeString([],{hour:'2-digit',minute:'2-digit'});
const fmtDate = (ts:number,_t:any) => {
  if (!ts || ts < 1000) return '';
  const d=new Date(ts), n=new Date();
  if(d.toDateString()===n.toDateString()) return _t('sidebar.today');
  n.setDate(n.getDate()-1); if(d.toDateString()===n.toDateString()) return _t('sidebar.yesterday');
  return d.toLocaleDateString([],{month:'short',day:'numeric'});
};

const fmtError = (e:any) => {
  const s = String(e?.message||e?.toString?.()||e||'Unknown error');
  if(s.includes('timeout')) return 'Request timed out. Please try again.';
  if(s.includes('network')||s.includes('fetch')) return 'Network error. Check your connection.';
  return s.substring(0,200);
};

interface Msg { id:string; role:'user'|'assistant'; content:string; ts:number; model?:string; }
interface Sess { id:string; title:string; ts:number; pinned?:boolean; archived?:boolean; }

const THEMES = { dark: {bg:'#090d15',fg:'#e4e8ee',card:'#111827',mu:'#0c1222',ac:'#1e293b',bo:'#1e293b',pr:'#6366f1',sub:'#64748b',inp:'#0f172a',hov:'#1e293b',code:'#0f172a'} as const,
  light: {bg:'#f8fafc',fg:'#0f172a',card:'#fff',mu:'#f1f5f9',ac:'#e2e8f0',bo:'#e2e8f0',pr:'#6366f1',sub:'#64748b',inp:'#f1f5f9',hov:'#e2e8f0',code:'#f1f5f9'} as const };

const estimateTokens = (text:string):number => { let t=0; for(const c of text){t+=c.charCodeAt(0)>127?2:1;} return Math.ceil(t/4); };

export default function App() {
  const { t:_, language:lang, setLanguage, languages } = useI18n();
  const [dark,setDark] = useState(true);
  const theme = dark ? THEMES.dark : THEMES.light;
  const [sessions,setSessions] = useState<Sess[]>([]);
  const [sid,setSid] = useState<string|null>(null);
  const [msgs,setMsgs] = useState<Msg[]>([]);
  const [input,setInput] = useState('');
  const [loading,setLoading] = useState(false);
  const [model,setModel] = useState('deepseek-v4-flash');
  const [search,setSearch] = useState('');
  const [showCfg,setShowCfg] = useState(false);
  const [files,setFiles] = useState<string[]>([]);
  const [darkMode,setDarkMode] = useState(true);

  const bottomRef = useRef<HTMLDivElement>(null);
  const fileRef = useRef<HTMLInputElement>(null);

  const invoke = useCallback(<T,>(cmd:string,args?:Record<string,unknown>):Promise<T>=>{
    const tauri = (window as any).__TAURI__;
    const fn = tauri?.core?.invoke || tauri?.invoke;
    if (fn) return fn(cmd, args);
    return Promise.resolve({} as T);
  }, []);

  useEffect(()=>{bottomRef.current?.scrollIntoView({behavior:'smooth'});},[msgs]);
  useEffect(()=>{invoke<Sess[]>('list_sessions').then(l=>{if(l?.length){setSessions(l);setSid(l[0].id);invoke<Msg[]>('get_messages',{sessionId:l[0].id}).then(m=>{if(m)setMsgs(m.map(x=>({...x,ts:x.ts||0})));});}}).catch(()=>{});},[]);

  const doSend = (text?:string) => {
    const t = (text||input).trim(); if(!t&&!files.length) return;
    const prompt = t||'Analyze files'; const msgId = Date.now().toString();
    const startTime = Date.now();
    setMsgs(p=>[...p,{id:msgId,role:'user',content:prompt,ts:Date.now()}]);
    setInput('');setLoading(true);
    return invoke<any>('chat',{req:{prompt,files:files.length?files:null,sessionId:sid}})
      .then((r:any)=>{
        const output = typeof r==='string'?r:(r?.output||'No response');
        setMsgs(p=>[...p,{id:(Date.now()+1).toString(),role:'assistant',content:output,ts:Date.now(),model}]);
        if(r?.session_id){setSid(r.session_id);invoke<Sess[]>('list_sessions').then(l=>{if(l)setSessions(l);});}
        try { new (window as any).Notification?.('Done',{body:prompt.substring(0,50)}); } catch {}
      })
      .catch((e:any)=>{setMsgs(p=>[...p,{id:(Date.now()+1).toString(),role:'assistant',content:'\u26a0\ufe0f '+fmtError(e),ts:Date.now()}]);})
      .finally(()=>{setLoading(false);setFiles([]);});
  };

  const onNew = async()=>{const r=await invoke<Sess>('create_session');if(r){setSessions(p=>[r,...p]);setSid(r.id);setMsgs([]);}};
  const onSwitch = async(id:string)=>{setSid(id);invoke<Msg[]>('get_messages',{sessionId:id}).then(m=>{if(m)setMsgs(m.map(x=>({...x,ts:x.ts||0})));});};
  const onDel = async(id:string)=>{invoke('delete_session',{sessionId:id});setSessions(p=>p.filter(s=>s.id!==id));if(sid===id){setSid(null);setMsgs([]);}};
  const onExport = async(fmt:string)=>{if(!sid)return;const r=await invoke<{content:string}>('export_conversation',{sessionId:sid,format:fmt});if(r?.content)navigator.clipboard.writeText(r.content);};

  const filtered = sessions.filter(s=>s.title.toLowerCase().includes(search.toLowerCase()));
  const sorted = [...filtered.filter(s=>s.pinned),...filtered.filter(s=>!s.pinned)].sort((a,b)=>b.ts-a.ts);

  const SUGGESTIONS = [
    {emoji:'\ud83d\udcbb',text:_('welcome.prompt1')},{emoji:'\ud83d\udc1b',text:_('welcome.prompt2')},{emoji:'\ud83d\udcdd',text:_('welcome.prompt3')},
    {emoji:'\ud83d\udd0d',text:_('welcome.prompt4')},{emoji:'\ud83d\udcca',text:_('welcome.prompt5')},{emoji:'\ud83c\udfa8',text:_('welcome.prompt6')},
  ];

  return (
    <div style={{display:'flex',height:'100vh',fontSize:14,background:theme.bg,color:theme.fg}}>
      <style>{`html,body,*{cursor:default!important}button,a,[role=button]{cursor:pointer!important}input,textarea{cursor:text!important}`}</style>
      {/* Sidebar */}
      <div style={{width:260,minWidth:260,background:theme.mu,borderRight:`1px solid ${theme.bo}`,display:'flex',flexDirection:'column'}}>
        <div style={{padding:'14px 16px 12px',borderBottom:`1px solid ${theme.bo}`}}>
          <div style={{display:'flex',alignItems:'center',gap:8,marginBottom:12}}>
            <div style={{width:28,height:28,borderRadius:8,background:'linear-gradient(135deg,#6366f1,#8b5cf6)',display:'flex',alignItems:'center',justifyContent:'center',fontSize:16}}>\u26a1</div>
            <div><div style={{fontWeight:800,fontSize:15}}>{_('app.name')}</div><div style={{fontSize:10,color:theme.sub}}>{_('app.version')}</div></div>
          </div>
          <button onClick={onNew} style={{width:'100%',padding:'10px 14px',borderRadius:10,border:`1px solid ${theme.bo}`,background:'transparent',color:theme.fg,cursor:'pointer',fontSize:13,display:'flex',alignItems:'center',gap:8,fontFamily:'inherit',fontWeight:500}}><S.Plus/> {_('sidebar.new')}</button>
        </div>
        <div style={{padding:'4px 8px'}}>
          <div style={{position:'relative'}}>
            <span style={{position:'absolute',left:10,top:'50%',transform:'translateY(-50%)',opacity:.4,display:'flex'}}><S.SearchIcon/></span>
            <input value={search} onChange={e=>setSearch(e.target.value)} placeholder={_('sidebar.search')} style={{width:'100%',padding:'8px 12px 8px 32px',borderRadius:8,border:`1px solid ${theme.bo}`,background:theme.inp,color:theme.fg,fontSize:12,fontFamily:'inherit',outline:'none'}}/>
          </div>
        </div>
        <div style={{flex:1,overflowY:'auto',padding:'2px 8px'}}>
          {sorted.length===0&&<div style={{textAlign:'center',color:theme.sub,fontSize:12,padding:20}}>{search?_('sidebar.noResults'):_('sidebar.noConversations')}</div>}
          {sorted.map(s=>(
            <div key={s.id} onClick={()=>onSwitch(s.id)} style={{padding:'9px 12px',borderRadius:8,cursor:'pointer',fontSize:13,marginBottom:1,background:s.id===sid?theme.ac:'transparent',display:'flex',alignItems:'center',gap:8}}>
              <div style={{flex:1,overflow:'hidden'}}>
                <div style={{fontWeight:500,overflow:'hidden',textOverflow:'ellipsis',whiteSpace:'nowrap',fontSize:13}}>{s.title}</div>
                <div style={{fontSize:10,color:theme.sub,marginTop:1}}>{fmtTime(s.ts)}</div>
              </div>
              <div style={{display:'flex',gap:2,opacity:0,flexShrink:0}} className="hover-show">
                <button onClick={e=>{e.stopPropagation();onDel(s.id);}} style={{background:'none',border:'none',color:theme.sub,cursor:'pointer',padding:'2px 3px',borderRadius:4}}><S.Trash/></button>
              </div>
            </div>
          ))}
        </div>
        <div style={{padding:8,borderTop:`1px solid ${theme.bo}`,display:'flex',flexDirection:'column',gap:4}}>
          {sid&&<><button onClick={()=>onExport('markdown')} style={{padding:'8px 12px',borderRadius:8,border:'none',background:'transparent',color:theme.sub,cursor:'pointer',fontSize:12,textAlign:'left',display:'flex',alignItems:'center',gap:8}}><S.Export/> {_('sidebar.exportMd')}</button>
          <button onClick={()=>onExport('json')} style={{padding:'8px 12px',borderRadius:8,border:'none',background:'transparent',color:theme.sub,cursor:'pointer',fontSize:12,textAlign:'left',display:'flex',alignItems:'center',gap:8}}><S.Export/> {_('sidebar.exportJson')}</button></>}
          <button onClick={()=>setShowCfg(true)} style={{padding:'8px 12px',borderRadius:8,border:'none',background:'transparent',color:theme.sub,cursor:'pointer',fontSize:12,textAlign:'left',display:'flex',alignItems:'center',gap:8}}><S.Gear/> {_('sidebar.settings')}</button>
        </div>
      </div>

      {/* Main */}
      <div style={{flex:1,display:'flex',flexDirection:'column',minWidth:0}}>
        {/* Header */}
        <div style={{padding:'8px 20px',borderBottom:`1px solid ${theme.bo}`,display:'flex',alignItems:'center',gap:12,minHeight:46,background:theme.bg}}>
          <select value={model} onChange={e=>setModel(e.target.value)} style={{padding:'4px 8px',borderRadius:6,border:`1px solid ${theme.bo}`,background:theme.inp,color:theme.fg,fontSize:12,fontFamily:'inherit',cursor:'pointer'}}>
            {MODELS.map(m=><option key={m} value={m}>{m}</option>)}
          </select>
          <div style={{flex:1}}/>
          {sid&&<span style={{fontSize:10,color:theme.sub,background:theme.ac,padding:'2px 8px',borderRadius:10}}>{msgs.length} msgs</span>}
          <button onClick={()=>setDark(!dark)} style={{background:'none',border:'none',cursor:'pointer',color:theme.sub,padding:4,borderRadius:6,display:'flex'}}>{dark?<S.Sun/>:<S.Moon/>}</button>
          <button onClick={()=>setShowCfg(true)} style={{background:'none',border:'none',cursor:'pointer',color:theme.sub,padding:4,borderRadius:6,display:'flex'}}><S.Gear/></button>
        </div>

        {/* Messages */}
        <div style={{flex:1,overflowY:'auto',padding:'16px 24px'}}>
          {msgs.length===0&&!loading?(
            <div style={{display:'flex',flexDirection:'column',alignItems:'center',justifyContent:'center',height:'100%',gap:16}}>
              <div style={{fontSize:40}}>\u26a1</div>
              <div style={{fontSize:20,fontWeight:700}}>HyperAgent Desktop</div>
              <div style={{color:theme.sub,fontSize:14,maxWidth:400,textAlign:'center'}}>{_('welcome.subtitle')}</div>
              <div style={{display:'grid',gridTemplateColumns:'1fr 1fr',gap:8,maxWidth:500}}>
                {SUGGESTIONS.map((s,i)=><button key={i} onClick={()=>doSend(s.text)} style={{padding:'12px 16px',borderRadius:10,border:`1px solid ${theme.bo}`,background:theme.card,cursor:'pointer',color:theme.fg,textAlign:'left',fontSize:12,fontFamily:'inherit'}}>{s.emoji} {s.text}</button>)}
              </div>
            </div>
          ):msgs.map((m,i)=>{
            const isUser = m.role==='user';
            const rendered = m.content
              .replace(/```(\w*)\n([\\s\\S]*?)```/g,'[code]')
              .replace(/\*\*(.+?)\*\*/g,'<b>$1</b>')
              .replace(/\*(.+?)\*/g,'<i>$1</i>');
            return (
            <div key={m.id} style={{marginBottom:16,display:'flex',flexDirection:'column',alignItems:isUser?'flex-end':'flex-start'}}>
              <div style={{maxWidth:'80%',padding:'10px 16px',borderRadius:14,borderBottomRightRadius:isUser?4:14,borderBottomLeftRadius:isUser?14:4,background:isUser?theme.pr:theme.card,color:isUser?'#fff':theme.fg,fontSize:13,lineHeight:1.7,whiteSpace:'pre-wrap',wordBreak:'break-word'}} dangerouslySetInnerHTML={{__html:rendered}}/>
              <div style={{fontSize:10,color:theme.sub,marginTop:3}}>{fmtTime(m.ts)}{m.model?` · ${m.model}`:''}{loading&&i===msgs.length-1?<span style={{animation:'pulse 1.5s infinite',opacity:.5}}> · Thinking...</span>:null}</div>
            </div>
          )})}
          <div ref={bottomRef}/>
        </div>

        {/* Input */}
        <div style={{padding:'12px 24px 16px',borderTop:`1px solid ${theme.bo}`}}>
          <div style={{display:'flex',gap:10,alignItems:'flex-end',background:theme.inp,borderRadius:14,padding:'8px 12px',border:`1px solid ${theme.bo}`}}>
            <textarea value={input} onChange={e=>setInput(e.target.value)} onKeyDown={e=>{if(e.key==='Enter'&&!e.shiftKey){e.preventDefault();doSend();}}}
              placeholder={loading?_('input.waiting'):_('input.placeholder')} rows={1}
              style={{flex:1,minHeight:36,maxHeight:150,resize:'none',border:'none',background:'transparent',color:theme.fg,padding:'6px 0',fontSize:14,fontFamily:'inherit',outline:'none'}}/>
            <button onClick={()=>doSend()} disabled={loading||(!input.trim()&&!files.length)}
              style={{padding:'9px 18px',borderRadius:10,border:'none',background:loading||(!input.trim()&&!files.length)?theme.ac:theme.pr,color:'#fff',cursor:'pointer',fontWeight:600,fontSize:14,display:'flex',alignItems:'center',gap:6,fontFamily:'inherit'}}><S.Send/> Send</button>
          </div>
          {input.length>0&&<div style={{textAlign:'right',fontSize:10,color:theme.sub,marginTop:4}}>~{estimateTokens(input)} tokens</div>}
        </div>
      </div>
    </div>
  );
}
