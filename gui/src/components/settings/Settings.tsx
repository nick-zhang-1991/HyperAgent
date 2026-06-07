import { useState } from 'react';

interface SettingsProps {
  isOpen: boolean;
  onClose: () => void;
  apiKey: string;
  model: string;
  baseUrl: string;
  onSave: (apiKey: string, model: string, baseUrl: string) => void;
}

export function Settings({
  isOpen,
  onClose,
  apiKey,
  model,
  baseUrl,
  onSave,
}: SettingsProps) {
  const [localApiKey, setLocalApiKey] = useState(apiKey);
  const [localModel, setLocalModel] = useState(model);
  const [localBaseUrl, setLocalBaseUrl] = useState(baseUrl);

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-background border rounded-lg p-6 w-full max-w-md space-y-4">
        <div className="flex items-center justify-between">
          <h2 className="text-xl font-semibold">Settings</h2>
          <button
            onClick={onClose}
            className="text-muted-foreground hover:text-foreground"
          >
            ✕
          </button>
        </div>

        <div className="space-y-4">
          <div className="space-y-2">
            <label className="text-sm font-medium">API Key</label>
            <input
              type="password"
              value={localApiKey}
              onChange={(e) => setLocalApiKey(e.target.value)}
              placeholder="sk-..."
              className="w-full px-3 py-2 text-sm border rounded-md bg-background"
            />
          </div>

          <div className="space-y-2">
            <label className="text-sm font-medium">Model</label>
            <select
              value={localModel}
              onChange={(e) => setLocalModel(e.target.value)}
              className="w-full px-3 py-2 text-sm border rounded-md bg-background"
            >
              <option value="gpt-4o">GPT-4o</option>
              <option value="gpt-4o-mini">GPT-4o Mini</option>
              <option value="claude-3-5-sonnet-20240620">Claude 3.5 Sonnet</option>
              <option value="deepseek-chat">DeepSeek Chat</option>
              <option value="qwen-turbo">Qwen Turbo</option>
            </select>
          </div>

          <div className="space-y-2">
            <label className="text-sm font-medium">Base URL</label>
            <input
              type="text"
              value={localBaseUrl}
              onChange={(e) => setLocalBaseUrl(e.target.value)}
              placeholder="https://api.openai.com/v1"
              className="w-full px-3 py-2 text-sm border rounded-md bg-background"
            />
          </div>

          <div className="flex gap-2 pt-4">
            <button
              onClick={() => onSave(localApiKey, localModel, localBaseUrl)}
              className="flex-1 bg-primary text-primary-foreground rounded-md px-4 py-2 text-sm font-medium hover:bg-primary/90"
            >
              Save
            </button>
            <button
              onClick={onClose}
              className="flex-1 border rounded-md px-4 py-2 text-sm font-medium hover:bg-accent"
            >
              Cancel
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
