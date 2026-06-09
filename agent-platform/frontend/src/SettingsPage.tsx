import React, { useState } from 'react';

interface Props {
  onSave: (m: any) => void;
  onClose: () => void;
  current: { provider: string; model: string; api_key: string; temperature: number };
}

export default function SettingsPage({ onSave, onClose, current }: Props) {
  const [provider, setProvider] = useState(current.provider);
  const [model, setModel] = useState(current.model);
  const [apiKey, setApiKey] = useState(current.api_key);
  const [temp, setTemp] = useState(current.temperature);

  return (
    <div className="p-6 max-w-md mx-auto">
      <h2 className="text-lg font-semibold mb-4">Model Settings</h2>
      <div className="space-y-4">
        <div>
          <label className="block text-xs text-gray-500 mb-1">Provider</label>
          <select value={provider} onChange={e => setProvider(e.target.value)}
            className="w-full px-3 py-2 rounded-lg bg-[#06060a] border border-gray-800 text-sm">
            {['openai','anthropic','deepseek','groq','together','local'].map(p => <option key={p} value={p}>{p}</option>)}
          </select>
        </div>
        <div>
          <label className="block text-xs text-gray-500 mb-1">Model</label>
          <input value={model} onChange={e => setModel(e.target.value)}
            className="w-full px-3 py-2 rounded-lg bg-[#06060a] border border-gray-800 text-sm" placeholder="gpt-4o" />
        </div>
        <div>
          <label className="block text-xs text-gray-500 mb-1">API Key</label>
          <input value={apiKey} onChange={e => setApiKey(e.target.value)} type="password"
            className="w-full px-3 py-2 rounded-lg bg-[#06060a] border border-gray-800 text-sm" placeholder="sk-..." />
        </div>
        <div>
          <label className="block text-xs text-gray-500 mb-1">Temperature: {temp.toFixed(1)}</label>
          <input type="range" min="0" max="2" step="0.1" value={temp}
            onChange={e => setTemp(parseFloat(e.target.value))}
            className="w-full accent-indigo-500" />
        </div>
        <div className="flex gap-2 pt-2">
          <button onClick={() => onSave({ provider, model, api_key: apiKey, temperature: temp })}
            className="flex-1 py-2 bg-indigo-600 hover:bg-indigo-500 rounded-lg text-sm">Save</button>
          <button onClick={onClose}
            className="flex-1 py-2 bg-gray-800 hover:bg-gray-700 rounded-lg text-sm">Cancel</button>
        </div>
      </div>
    </div>
  );
}
