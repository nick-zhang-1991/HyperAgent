import { useRef, useEffect, useState, useCallback } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { Prism as SyntaxHighlighter } from 'react-syntax-highlighter';
import { oneDark } from 'react-syntax-highlighter/dist/esm/styles/prism';
import {
  Paperclip, Send, X, Copy, Check, RefreshCw, Square,
  Pencil, Mic, MicOff, Sun, Moon, ZoomIn, ZoomOut,
  Bookmark, ChevronUp, Volume2,
} from 'lucide-react';

// ── Types ───────────────────────────────────────────────────────

interface Message {
  id: string;
  role: 'user' | 'assistant' | 'system' | 'assistant-streaming';
  content: string;
  files?: string[];
  timestamp: number;
  tokens?: { input: number; output: number };
}

interface ChatAreaProps {
  messages: Message[];
  isLoading: boolean;
  onSendMessage: (prompt: string, files?: string[], editingId?: string) => void;
  onStopGeneration: () => void;
  onRetry: () => void;
  _sessionId?: string | null;  onVoiceInput?: () => void;
  isListening?: boolean;
  theme: 'light' | 'dark';
  onToggleTheme: () => void;
  sessionId?: string | null;
}

// ── Inline code copy hook ───────────────────────────────────────

function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      className="absolute top-2 right-2 p-1 rounded bg-gray-700 hover:bg-gray-600 opacity-0 group-hover:opacity-100 transition-opacity z-10"
      onClick={() => {
        navigator.clipboard.writeText(text).then(() => {
          setCopied(true);
          setTimeout(() => setCopied(false), 2000);
        });
      }}
      title="Copy code"
    >
      {copied ? <Check className="h-3.5 w-3.5 text-green-400" /> : <Copy className="h-3.5 w-3.5 text-gray-300" />}
    </button>
  );
}

// ── Main Component ──────────────────────────────────────────────

export function ChatArea({
  messages,
  isLoading,
  onSendMessage,
  onStopGeneration,
  onRetry,
  theme,
  onToggleTheme,
}: ChatAreaProps) {
  const bottomRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [attachedFiles, setAttachedFiles] = useState<File[]>([]);
  const [isDragging, setIsDragging] = useState(false);
  const [autoScroll, setAutoScroll] = useState(true);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editText, setEditText] = useState('');
  const [fontSize, setFontSize] = useState(14);
  const [isListening, setIsListening] = useState(false);
  const [showTemplates, setShowTemplates] = useState(false);
  const [templates] = useState([
    { emoji: '💻', label: 'Code review', prompt: 'Review this code and suggest improvements:' },
    { emoji: '🐛', label: 'Debug', prompt: 'Help me debug this issue:' },
    { emoji: '📝', label: 'Write docs', prompt: 'Write documentation for:' },
    { emoji: '🔍', label: 'Explain', prompt: 'Explain how this works:' },
    { emoji: '🧪', label: 'Write tests', prompt: 'Write unit tests for:' },
    { emoji: '📊', label: 'Analyze', prompt: 'Analyze this data and give insights:' },
    { emoji: '🌐', label: 'Translate', prompt: 'Translate this to Chinese:' },
    { emoji: '✂️', label: 'Summarize', prompt: 'Summarize the following:' },
  ]);

  // Voice input
  const toggleVoice = useCallback(() => {
    const SpeechRecognition = (window as any).SpeechRecognition || (window as any).webkitSpeechRecognition;
    if (!SpeechRecognition) return alert('Speech recognition not available in this browser');
    
    if (isListening) {
      setIsListening(false);
      return;
    }
    
    const recognition = new SpeechRecognition();
    recognition.continuous = false;
    recognition.interimResults = true;
    recognition.lang = 'en-US';
    setIsListening(true);
    
    recognition.onresult = (event: any) => {
      let transcript = '';
      for (let i = 0; i < event.results.length; i++) {
        transcript += event.results[i][0].transcript;
      }
      if (inputRef.current) {
        inputRef.current.value = transcript;
        inputRef.current.style.height = 'auto';
        inputRef.current.style.height = Math.min(inputRef.current.scrollHeight, 200) + 'px';
      }
    };
    
    recognition.onerror = () => setIsListening(false);
    recognition.onend = () => setIsListening(false);
    recognition.start();
  }, [isListening]);

  // Auto-scroll
  useEffect(() => {
    if (autoScroll) {
      bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
    }
  }, [messages, autoScroll]);

  // Keyboard shortcuts
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      // Ctrl+N: new session (handled in App)
      // Ctrl+Enter: send message
      if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        handleSend();
      }
      // Ctrl++ / Ctrl+-: font zoom
      if ((e.metaKey || e.ctrlKey) && e.key === '=') {
        e.preventDefault();
        setFontSize(prev => Math.min(prev + 1, 20));
      }
      if ((e.metaKey || e.ctrlKey) && e.key === '-') {
        e.preventDefault();
        setFontSize(prev => Math.max(prev - 1, 10));
      }
      // Escape: cancel edit
      if (e.key === 'Escape' && editingId) {
        setEditingId(null);
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [editingId, attachedFiles]);

  // Scroll detection
  const handleScroll = useCallback((e: React.UIEvent<HTMLDivElement>) => {
    const el = e.currentTarget;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 50;
    setAutoScroll(atBottom);
  }, []);

  const handleSend = () => {
    const text = inputRef.current?.value.trim();
    if (editingId) {
      if (editText.trim()) {
        onSendMessage(editText.trim(), undefined, editingId);
        setEditingId(null);
        setEditText('');
      }
      return;
    }
    if ((!text && attachedFiles.length === 0) || isLoading) return;
    const fileNames = attachedFiles.map(f => (f as any).path || f.name);
    onSendMessage(text || 'Analyze attached files', fileNames.length > 0 ? fileNames : undefined);
    if (inputRef.current) inputRef.current.value = '';
    setAttachedFiles([]);
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey && !e.ctrlKey && !e.metaKey) {
      e.preventDefault();
      handleSend();
    }
  };

  const handleEdit = (msg: Message) => {
    setEditingId(msg.id);
    setEditText(msg.content);
    setTimeout(() => inputRef.current?.focus(), 0);
  };

  const cancelEdit = () => {
    setEditingId(null);
    setEditText('');
  };

  return (
    <div className="flex flex-col h-full relative" onDragOver={(e) => { e.preventDefault(); setIsDragging(true); }} onDragLeave={() => setIsDragging(false)} onDrop={(e) => { e.preventDefault(); setIsDragging(false); if (e.dataTransfer.files) setAttachedFiles(prev => [...prev, ...Array.from(e.dataTransfer.files)]); }}>
      {/* Drag overlay */}
      {isDragging && (
        <div className="absolute inset-0 bg-primary/10 border-2 border-dashed border-primary z-50 flex items-center justify-center rounded-lg m-2">
          <div className="text-center">
            <Paperclip className="h-10 w-10 mx-auto text-primary mb-2" />
            <p className="text-lg font-semibold text-primary">Drop files here</p>
            <p className="text-xs text-muted-foreground mt-1">Images · PDFs · Code · Documents</p>
          </div>
        </div>
      )}

      {/* Messages */}
      <div className="flex-1 overflow-y-auto p-4 space-y-4" style={{ fontSize: `${fontSize}px` }} onScroll={handleScroll}>
        {messages.length === 0 && (
          <div className="flex flex-col items-center justify-center h-full text-muted-foreground pt-16">
            <div className="text-6xl mb-4">⚡</div>
            <h2 className="text-2xl font-semibold mb-2">HyperAgent Desktop</h2>
            <p className="text-sm mb-4">Your all-in-one AI assistant</p>
            <div className="flex flex-wrap justify-center gap-2 text-xs">
              {['💻 Coding', '📝 Writing', '🔍 Research', '📊 Analysis', '🎨 Creative', '🐛 Debug'].map(tag => (
                <span key={tag} className="bg-muted px-3 py-1.5 rounded-full">{tag}</span>
              ))}
            </div>
            <div className="mt-6 text-xs text-muted-foreground space-y-1 text-center">
              <p><kbd className="px-1.5 py-0.5 bg-muted rounded text-[10px]">Ctrl+N</kbd> New session</p>
              <p><kbd className="px-1.5 py-0.5 bg-muted rounded text-[10px]">Ctrl+Enter</kbd> Send</p>
              <p><kbd className="px-1.5 py-0.5 bg-muted rounded text-[10px]">Ctrl++/-</kbd> Zoom</p>
            </div>
          </div>
        )}

        {messages.map((msg, idx) => {
          const isLastAssistant = msg.role === 'assistant' && idx === messages.length - 1;
          return (
            <div key={msg.id} className={`flex ${msg.role === 'user' ? 'justify-end' : 'justify-start'} group/message`}>
              <div className={`max-w-[88%] rounded-xl px-4 py-3 relative ${
                msg.role === 'user'
                  ? 'bg-primary text-primary-foreground'
                  : msg.role === 'system'
                  ? 'bg-amber-50 dark:bg-amber-950/20 border border-amber-200 dark:border-amber-800'
                  : 'bg-muted'
              }`}>
                {/* Files */}
                {msg.files && msg.files.length > 0 && (
                  <div className="flex flex-wrap gap-1.5 mb-2">
                    {msg.files.map((f, i) => (
                      <span key={i} className="inline-flex items-center gap-1 text-xs bg-background/20 px-2 py-1 rounded-full">
                        📎 {f.split('/').pop() || f}
                      </span>
                    ))}
                  </div>
                )}

                {/* Content */}
                {editingId === msg.id ? (
                  <div className="w-full">
                    <textarea
                      value={editText}
                      onChange={e => setEditText(e.target.value)}
                      className="w-full min-h-[80px] bg-background/30 rounded-md border border-input px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-ring"
                      autoFocus
                      onKeyDown={e => {
                        if (e.key === 'Escape') cancelEdit();
                        if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) { e.preventDefault(); handleSend(); }
                      }}
                    />
                    <div className="flex justify-end gap-2 mt-1">
                      <button onClick={cancelEdit} className="text-xs px-2 py-1 rounded hover:bg-background/30">Cancel</button>
                      <button onClick={handleSend} className="text-xs px-2 py-1 rounded bg-primary/30 hover:bg-primary/50">Send</button>
                    </div>
                  </div>
                ) : (
                  <div className="prose prose-sm dark:prose-invert max-w-none break-words
                    [&_pre]:!bg-gray-900 [&_pre]:!text-gray-100 [&_pre]:!rounded-lg [&_pre]:!p-4 [&_pre]:!overflow-x-auto [&_pre]:!my-2
                    [&_code]:!text-[0.85em] [&_code:not(pre_code)]:!bg-muted-foreground/20 [&_code:not(pre_code)]:!px-1.5 [&_code:not(pre_code)]:!py-0.5 [&_code:not(pre_code)]:!rounded
                    [&_table]:!border-collapse [&_th]:!border [&_th]:!px-3 [&_th]:!py-1 [&_td]:!border [&_td]:!px-3 [&_td]:!py-1
                    [&_blockquote]:!border-l-4 [&_blockquote]:!border-muted-foreground/30 [&_blockquote]:!pl-4 [&_blockquote]:!italic">
                    <ReactMarkdown
                      remarkPlugins={[remarkGfm]}
                      components={{
                        code({ node, className, children, ...props }) {
                          const match = /language-(\w+)/.exec(className || '');
                          const codeStr = String(children).replace(/\n$/, '');
                          if (match) {
                            return (
                              <div className="group relative">
                                <div className="flex items-center justify-between bg-gray-800 rounded-t-lg px-4 py-1.5 text-xs text-gray-400">
                                  <span>{match[1]}</span>
                                  <CopyButton text={codeStr} />
                                </div>
                                <SyntaxHighlighter
                                  style={oneDark}
                                  language={match[1]}
                                  PreTag="div"
                                  customStyle={{ margin: 0, borderTopLeftRadius: 0, borderTopRightRadius: 0 }}
                                >
                                  {codeStr}
                                </SyntaxHighlighter>
                              </div>
                            );
                          }
                          return <code className={className} {...props}>{children}</code>;
                        },
                      }}
                    >
                      {msg.content}
                    </ReactMarkdown>
                  </div>
                )}

                {/* Token info */}
                {msg.tokens && (
                  <div className="flex items-center gap-2 mt-2 text-[10px] opacity-40">
                    <span>📥 {msg.tokens.input}</span>
                    <span>📤 {msg.tokens.output}</span>
                  </div>
                )}

                {/* Timestamp + Actions */}
                <div className="flex items-center justify-between mt-1.5">
                  <span className="text-[10px] opacity-40">
                    {new Date(msg.timestamp).toLocaleString()}
                  </span>
                  {(msg.role === 'user' || (msg.role === 'assistant' && idx > 0)) && (
                    <div className="flex items-center gap-1 opacity-0 group-hover/message:opacity-100 transition-opacity">
                      {/* Copy */}
                      <button onClick={() => navigator.clipboard.writeText(msg.content)} className="p-1 rounded hover:bg-background/20" title="Copy"><Copy className="h-3 w-3" /></button>
                      {/* TTS for assistant messages */}
                      {msg.role === 'assistant' && (
                        <button onClick={() => { const u = new SpeechSynthesisUtterance(msg.content); u.lang = 'en-US'; speechSynthesis.speak(u); }} className="p-1 rounded hover:bg-background/20" title="Read aloud"><Volume2 className="h-3 w-3" /></button>
                      )}
                      {/* Edit (user messages) */}
                      {msg.role === 'user' && !editingId && (
                        <button onClick={() => handleEdit(msg)} className="p-1 rounded hover:bg-background/20" title="Edit">
                          <Pencil className="h-3 w-3" />
                        </button>
                      )}
                    </div>
                  )}
                </div>

                {/* Regenerate on last assistant message */}
                {isLastAssistant && !isLoading && msg.role === 'assistant' && (
                  <button
                    onClick={onRetry}
                    className="mt-2 text-xs text-muted-foreground hover:text-foreground flex items-center gap-1"
                  >
                    <RefreshCw className="h-3 w-3" /> Regenerate
                  </button>
                )}
              </div>
            </div>
          );
        })}

        {/* Loading */}
        {isLoading && (
          <div className="flex justify-start">
            <div className="bg-muted rounded-xl px-4 py-3 flex items-center gap-3">
              <div className="flex space-x-1">
                <div className="w-2 h-2 bg-foreground/30 rounded-full animate-bounce" />
                <div className="w-2 h-2 bg-foreground/30 rounded-full animate-bounce" style={{ animationDelay: '0.15s' }} />
                <div className="w-2 h-2 bg-foreground/30 rounded-full animate-bounce" style={{ animationDelay: '0.3s' }} />
              </div>
              <button onClick={onStopGeneration} className="p-1 rounded hover:bg-background/30" title="Stop generating">
                <Square className="h-3.5 w-3.5 text-red-400" />
              </button>
            </div>
          </div>
        )}

        <div ref={bottomRef} />
      </div>

      {/* Attached files preview */}
      {attachedFiles.length > 0 && (
        <div className="px-4 py-2 border-t bg-muted/30 flex flex-wrap gap-1.5 items-center">
          <span className="text-[10px] text-muted-foreground mr-1">Attached:</span>
          {attachedFiles.map((file, i) => {
            const isImage = file.type.startsWith('image/');
            const [previewUrl, setPreviewUrl] = useState<string | null>(null);
            if (isImage && !previewUrl) {
              const reader = new FileReader();
              reader.onload = () => setPreviewUrl(reader.result as string);
              reader.readAsDataURL(file);
            }
            return (
              <span key={i} className="inline-flex items-center gap-1.5 text-xs bg-background border rounded-full px-2 py-1 group/att">
                {isImage && previewUrl ? (
                  <img src={previewUrl} alt="preview" className="w-4 h-4 rounded object-cover" />
                ) : (
                  <span>📎</span>
                )}
                <span>{file.name.length > 25 ? file.name.slice(0, 25) + '...' : file.name}</span>
                <button
                  onClick={() => setAttachedFiles(prev => prev.filter((_, j) => j !== i))}
                  className="opacity-0 group-hover/att:opacity-100 hover:text-red-500 ml-0.5 transition-opacity"
                >
                  <X className="h-3 w-3" />
                </button>
              </span>
            );
          })}
          <button onClick={() => setAttachedFiles([])} className="text-[10px] text-muted-foreground hover:text-foreground ml-auto">Clear all</button>
        </div>
      )}

      {/* Input area */}
      <div className="border-t bg-background p-3">
        {editingId && (
          <div className="text-xs text-muted-foreground mb-2 flex items-center gap-2">
            <Pencil className="h-3 w-3" /> Editing message
            <button onClick={cancelEdit} className="hover:text-foreground">(cancel)</button>
          </div>
        )}
        <div className="flex gap-2 items-end">
          {/* Theme toggle */}
          <button onClick={onToggleTheme} className="p-2 hover:bg-accent rounded text-muted-foreground shrink-0" title="Toggle theme">
            {theme === 'dark' ? <Sun className="h-4 w-4" /> : <Moon className="h-4 w-4" />}
          </button>

          {/* File attach */}
          <button onClick={() => fileInputRef.current?.click()} className="p-2 hover:bg-accent rounded text-muted-foreground shrink-0" title="Attach files">
            <Paperclip className="h-4 w-4" />
          </button>
          <input ref={fileInputRef} type="file" multiple className="hidden" onChange={e => { const fl = e.target.files; if (fl) setAttachedFiles(prev => [...prev, ...Array.from(fl)]); }} accept="image/*,.pdf,.txt,.md,.rs,.py,.js,.ts,.html,.css,.json,.yaml,.toml,.xml,.csv" />

          {/* Voice input */}
          <button
            onClick={toggleVoice}
            className={`p-2 rounded shrink-0 transition-colors ${isListening ? 'bg-red-100 text-red-500 dark:bg-red-900/30' : 'hover:bg-accent text-muted-foreground'}`}
            title={isListening ? 'Stop listening' : 'Voice input'}
          >
            {isListening ? <MicOff className="h-4 w-4" /> : <Mic className="h-4 w-4" />}
          </button>

          {/* Prompt templates */}
          <div className="relative shrink-0">
            <button
              onClick={() => setShowTemplates(!showTemplates)}
              className={`p-2 rounded transition-colors ${showTemplates ? 'bg-accent' : 'hover:bg-accent text-muted-foreground'}`}
              title="Prompt templates"
            >
              <Bookmark className="h-4 w-4" />
            </button>
            {showTemplates && (
              <div className="absolute bottom-full left-0 mb-1 w-52 bg-popover border rounded-xl shadow-xl p-1.5 z-50">
                <div className="flex items-center justify-between px-2 py-1 text-xs text-muted-foreground">
                  <span>Templates</span>
                  <button onClick={() => setShowTemplates(false)} className="hover:text-foreground"><ChevronUp className="h-3 w-3" /></button>
                </div>
                {templates.map((t, i) => (
                  <button
                    key={i}
                    onClick={() => {
                      if (inputRef.current) {
                        inputRef.current.value = t.prompt + ' ';
                        inputRef.current.focus();
                        inputRef.current.style.height = 'auto';
                        inputRef.current.style.height = Math.min(inputRef.current.scrollHeight, 200) + 'px';
                      }
                      setShowTemplates(false);
                    }}
                    className="w-full text-left text-xs px-2 py-1.5 rounded-lg hover:bg-accent flex items-center gap-2"
                  >
                    <span>{t.emoji}</span>
                    <span>{t.label}</span>
                  </button>
                ))}
              </div>
            )}
          </div>

          {/* Text input */}
          <textarea
            ref={inputRef}
            placeholder={editingId ? 'Edit your message...' : isLoading ? 'Generating...' : 'Type a message... (Enter to send, Shift+Enter for new line)'}
            className="flex-1 min-h-[44px] max-h-[200px] resize-none rounded-xl border border-input bg-background px-4 py-2.5 text-sm placeholder:text-muted-foreground focus:outline-none focus:ring-2 focus:ring-ring disabled:opacity-50 transition-all"
            rows={1}
            onKeyDown={handleKeyDown}
            disabled={isLoading}
            onInput={e => { const el = e.currentTarget; el.style.height = 'auto'; el.style.height = Math.min(el.scrollHeight, 200) + 'px'; }}
          />

          {/* Font size */}
          <div className="flex flex-col shrink-0">
            <button onClick={() => setFontSize(prev => Math.min(prev + 1, 20))} className="p-1 hover:bg-accent rounded text-muted-foreground" title="Zoom in">
              <ZoomIn className="h-3 w-3" />
            </button>
            <button onClick={() => setFontSize(prev => Math.max(prev - 1, 10))} className="p-1 hover:bg-accent rounded text-muted-foreground" title="Zoom out">
              <ZoomOut className="h-3 w-3" />
            </button>
          </div>

          {/* Send / Stop button */}
          {isLoading ? (
            <button onClick={onStopGeneration} className="inline-flex items-center justify-center rounded-xl bg-red-500 p-2.5 text-white hover:bg-red-600 shrink-0 transition-colors" title="Stop generating">
              <Square className="h-4 w-4" />
            </button>
          ) : (
            <button onClick={handleSend} className="inline-flex items-center justify-center rounded-xl bg-primary p-2.5 text-primary-foreground hover:bg-primary/90 disabled:opacity-50 shrink-0 transition-colors" disabled={!editingId && attachedFiles.length === 0} title="Send">
              <Send className="h-4 w-4" />
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
