export interface Message {
  id: string;
  role: 'user' | 'assistant';
  content: string;
  image?: string;
  timestamp: number;
}

export interface ChatRequest {
  prompt: string;
  imagePath?: string;
  mode?: string;
}

export interface ChatResponse {
  response: string;
  success: boolean;
  error?: string;
}

// Web API types (server mode)
export interface ApiChatRequest {
  message: string;
  session_id?: string;
}

export interface ApiChatResponse {
  response: string;
  session_id: string;
}

export interface ApiHealthResponse {
  status: string;
  version: string;
  name: string;
}
