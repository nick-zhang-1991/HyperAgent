import { createApi, fetchBaseQuery } from '@reduxjs/toolkit/query/react';
import type { ChatRequest, ChatResponse } from '@/types';

export const API_BASE = import.meta.env.VITE_API_URL || 'http://127.0.0.1:3000';

export interface ChatStreamResponse {
  response: string;
  session_id: string;
}

export async function sendChatMessage(
  message: string,
  sessionId?: string
): Promise<ChatStreamResponse> {
  const res = await fetch(`${API_BASE}/api/chat`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ message, session_id: sessionId }),
  });
  if (!res.ok) {
    throw new Error(`API error: ${res.status}`);
  }
  return res.json();
}

export async function checkHealth(): Promise<{ status: string; version: string }> {
  const res = await fetch(`${API_BASE}/api/health`);
  if (!res.ok) throw new Error(`Health check failed: ${res.status}`);
  return res.json();
}

export const { useChatMutation, useHealthQuery } = api;
