import { useState } from 'react';
import {
  MessageSquarePlus,
  Settings,
  Github,
  Download,
  Trash2,
  ChevronDown,
  Search,
} from 'lucide-react';
import { Button } from '../ui/button';

interface SessionInfo {
  id: string;
  title: string;
  message_count: number;
  updated_at: number;
  mode: string;
}

interface SidebarProps {
  sessions: SessionInfo[];
  currentSessionId: string | null;
  onNewSession: () => void;
  onSwitchSession: (id: string) => void;
  onDeleteSession: (id: string) => void;
  onExport: (format: 'markdown' | 'json') => void;
  onOpenSettings: () => void;
}

export function Sidebar({
  sessions,
  currentSessionId,
  onNewSession,
  onSwitchSession,
  onDeleteSession,
  onExport,
  onOpenSettings,
}: SidebarProps) {
  const [showExportMenu, setShowExportMenu] = useState(false);
  const [searchQuery, setSearchQuery] = useState('');

  const filtered = sessions.filter(s =>
    s.title.toLowerCase().includes(searchQuery.toLowerCase())
  );

  const sorted = [...filtered].sort((a, b) => b.updated_at - a.updated_at);

  return (
    <div className="w-60 h-screen bg-muted/30 border-r flex flex-col shrink-0">
      {/* Header */}
      <div className="p-3 border-b">
        <h1 className="text-sm font-bold flex items-center gap-1.5">
          <span>⚡</span>
          HyperAgent
        </h1>
      </div>

      {/* New chat + search */}
      <div className="p-2 space-y-1.5">
        <Button
          onClick={onNewSession}
          className="w-full justify-start gap-2 text-xs h-8"
          variant="outline"
        >
          <MessageSquarePlus className="h-3.5 w-3.5" />
          New Conversation
        </Button>
        <div className="relative">
          <Search className="absolute left-2 top-1/2 -translate-y-1/2 h-3 w-3 text-muted-foreground" />
          <input
            type="text"
            placeholder="Search conversations..."
            value={searchQuery}
            onChange={e => setSearchQuery(e.target.value)}
            className="w-full text-xs pl-6 pr-2 py-1.5 rounded-md border border-input bg-background focus:outline-none focus:ring-1 focus:ring-ring"
          />
        </div>
      </div>

      {/* Session list */}
      <div className="flex-1 overflow-y-auto p-1">
        {sorted.length === 0 && (
          <div className="text-xs text-muted-foreground text-center py-8">
            {searchQuery ? 'No matching conversations' : 'No conversations yet'}
          </div>
        )}
        {sorted.map(session => {
          const isActive = session.id === currentSessionId;
          const modeIcon =
            session.mode === 'code' ? '💻' :
            session.mode === 'debug' ? '🐛' :
            session.mode === 'search' ? '🔍' : '💬';

          return (
            <div
              key={session.id}
              className={`group flex items-center gap-1.5 px-2 py-1.5 my-0.5 rounded text-xs cursor-pointer transition-colors ${
                isActive
                  ? 'bg-accent text-accent-foreground'
                  : 'hover:bg-accent/50'
              }`}
              onClick={() => onSwitchSession(session.id)}
            >
              <span className="shrink-0">{modeIcon}</span>
              <span className="flex-1 truncate">{session.title || 'Untitled'}</span>
              <button
                className="opacity-0 group-hover:opacity-100 p-0.5 hover:bg-red-100 rounded shrink-0"
                onClick={e => {
                  e.stopPropagation();
                  if (window.confirm('Delete this conversation?')) {
                    onDeleteSession(session.id);
                  }
                }}
                title="Delete"
              >
                <Trash2 className="h-3 w-3 text-muted-foreground hover:text-red-500" />
              </button>
            </div>
          );
        })}
      </div>

      {/* Footer actions */}
      <div className="p-2 border-t space-y-1">
        {/* Export */}
        {currentSessionId && (
          <div className="relative">
            <Button
              onClick={() => setShowExportMenu(!showExportMenu)}
              className="w-full justify-start gap-2 text-xs h-8"
              variant="ghost"
            >
              <Download className="h-3.5 w-3.5" />
              Export
              <ChevronDown className="h-3 w-3 ml-auto" />
            </Button>
            {showExportMenu && (
              <div className="absolute bottom-full left-0 w-full mb-1 bg-popover border rounded-md shadow-lg p-1">
                <button
                  onClick={() => { onExport('markdown'); setShowExportMenu(false); }}
                  className="w-full text-left text-xs px-2 py-1 rounded hover:bg-accent"
                >
                  📝 Markdown
                </button>
                <button
                  onClick={() => { onExport('json'); setShowExportMenu(false); }}
                  className="w-full text-left text-xs px-2 py-1 rounded hover:bg-accent"
                >
                  📋 JSON
                </button>
              </div>
            )}
          </div>
        )}

        <Button
          onClick={onOpenSettings}
          className="w-full justify-start gap-2 text-xs h-8"
          variant="ghost"
        >
          <Settings className="h-3.5 w-3.5" />
          Settings
        </Button>

        <a
          href="https://github.com/nick-zhang-1991/HyperAgent"
          target="_blank"
          rel="noopener noreferrer"
          className="flex items-center gap-2 px-2 py-1.5 text-xs text-muted-foreground hover:text-foreground rounded"
        >
          <Github className="h-3.5 w-3.5" />
          GitHub
        </a>
      </div>
    </div>
  );
}
