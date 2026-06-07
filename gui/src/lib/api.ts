import { createApi, fetchBaseQuery } from '@reduxjs/toolkit/query/react';
import type { ChatRequest, ChatResponse } from '@/types';

export const api = createApi({
  baseQuery: fetchBaseQuery({ baseUrl: 'http://localhost:3000' }),
  endpoints: (builder) => ({
    chat: builder.mutation<ChatResponse, ChatRequest>({
      query: (body) => ({
        url: '/api/chat',
        method: 'POST',
        body,
      }),
    }),
    health: builder.query<{ status: string }, void>({
      query: () => '/api/health',
    }),
  }),
});

export const { useChatMutation, useHealthQuery } = api;
