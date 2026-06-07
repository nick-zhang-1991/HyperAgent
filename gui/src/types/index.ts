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
