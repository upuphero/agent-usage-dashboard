import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { App } from './app/App';
import { createClient } from './app/createClient';
import { UsageProvider } from './app/UsageContext';
import './styles.css';

const { client, demo, scenario } = createClient(window.location.search);
const cache = new QueryClient({ defaultOptions: { queries: { retry: false, staleTime: 30_000, refetchOnWindowFocus: false }, mutations: { retry: false } } });
createRoot(document.getElementById('root')!).render(
  <StrictMode><QueryClientProvider client={cache}><UsageProvider client={client}><App demo={demo} scenario={scenario} /></UsageProvider></QueryClientProvider></StrictMode>,
);
