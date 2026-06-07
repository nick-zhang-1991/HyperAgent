import { useState } from 'react';
import { X, Maximize2, Download, PanelRightClose } from 'lucide-react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { Prism as SyntaxHighlighter } from 'react-syntax-highlighter';
import { oneDark } from 'react-syntax-highlighter/dist/esm/styles/prism';

interface CanvasProps {
  content: string;
  isOpen: boolean;
  onClose: () => void;
}

export function Canvas({ content, isOpen, onClose }: CanvasProps) {
  const [isFullscreen, setIsFullscreen] = useState(false);

  if (!isOpen) return null;

  return (
    <div className={`border-l bg-background flex flex-col ${isFullscreen ? 'fixed inset-0 z-50' : 'w-[45%] min-w-[400px]'}`}>
      <div className="flex items-center justify-between px-3 py-2 border-b bg-muted/30 shrink-0">
        <span className="text-xs font-medium flex items-center gap-2">
          <PanelRightClose className="h-3.5 w-3.5" />
          Canvas
        </span>
        <div className="flex items-center gap-1">
          <button
            onClick={() => navigator.clipboard.writeText(content)}
            className="p-1 hover:bg-accent rounded text-xs"
            title="Copy"
          >
            <Download className="h-3 w-3" />
          </button>
          <button
            onClick={() => setIsFullscreen(!isFullscreen)}
            className="p-1 hover:bg-accent rounded"
            title={isFullscreen ? 'Exit fullscreen' : 'Fullscreen'}
          >
            <Maximize2 className="h-3 w-3" />
          </button>
          <button onClick={onClose} className="p-1 hover:bg-accent rounded">
            <X className="h-3 w-3" />
          </button>
        </div>
      </div>
      <div className="flex-1 overflow-y-auto p-4 text-sm">
        <div className="prose prose-sm dark:prose-invert max-w-none
          [&_pre]:!bg-gray-900 [&_pre]:!text-gray-100 [&_pre]:!rounded-lg [&_pre]:!p-4 [&_pre]:!overflow-x-auto
          [&_code]:!text-[0.85em] [&_table]:!border-collapse [&_th]:!border [&_td]:!border">
          <ReactMarkdown
            remarkPlugins={[remarkGfm]}
            components={{
              code({ node, className, children, ...props }) {
                const match = /language-(\w+)/.exec(className || '');
                const codeStr = String(children).replace(/\n$/, '');
                if (match) {
                  return (
                    <SyntaxHighlighter style={oneDark} language={match[1]} PreTag="div" customStyle={{ borderRadius: '0.5rem' }}>
                      {codeStr}
                    </SyntaxHighlighter>
                  );
                }
                return <code className={className} {...props}>{children}</code>;
              },
            }}
          >
            {content}
          </ReactMarkdown>
        </div>
      </div>
    </div>
  );
}
