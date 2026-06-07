import { X, Check, RotateCcw } from 'lucide-react';

interface DiffPreviewProps {
  filePath: string;
  oldContent: string;
  newContent: string;
  onApply: () => void;
  onReject: () => void;
}

export function DiffPreview({ filePath, oldContent, newContent, onApply, onReject }: DiffPreviewProps) {
  const oldLines = oldContent.split('\n');
  const newLines = newContent.split('\n');
  const maxLines = Math.max(oldLines.length, newLines.length);

  return (
    <div className="border rounded-lg overflow-hidden my-2 text-xs font-mono">
      <div className="flex items-center justify-between px-3 py-1.5 bg-muted/50 border-b">
        <span className="font-medium">{filePath}</span>
        <div className="flex items-center gap-1">
          <button
            onClick={onApply}
            className="inline-flex items-center gap-1 px-2 py-0.5 rounded bg-green-600 text-white hover:bg-green-700 text-[11px]"
          >
            <Check className="h-3 w-3" /> Apply
          </button>
          <button
            onClick={onReject}
            className="inline-flex items-center gap-1 px-2 py-0.5 rounded bg-muted hover:bg-accent text-[11px]"
          >
            <X className="h-3 w-3" /> Reject
          </button>
          <button
            onClick={onReject}
            className="inline-flex items-center gap-1 px-2 py-0.5 rounded bg-muted hover:bg-accent text-[11px]"
          >
            <RotateCcw className="h-3 w-3" /> Revert
          </button>
        </div>
      </div>
      <div className="overflow-x-auto max-h-[300px] overflow-y-auto">
        {Array.from({ length: maxLines }, (_, i) => {
          const oldLine = oldLines[i];
          const newLine = newLines[i];
          if (oldLine === newLine) {
            return <div key={i} className="px-3 py-0.5 text-muted-foreground">  {i + 1}│ {oldLine || ''}</div>;
          }
          return (
            <div key={i}>
              {oldLine !== undefined && (
                <div className="px-3 py-0.5 bg-red-500/10 text-red-600 dark:text-red-400">
                  - {i + 1}│ {oldLine}
                </div>
              )}
              {newLine !== undefined && newLine !== oldLine && (
                <div className="px-3 py-0.5 bg-green-500/10 text-green-600 dark:text-green-400">
                  + {i + 1}│ {newLine}
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
