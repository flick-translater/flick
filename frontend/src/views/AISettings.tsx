import { useState } from 'react';
import { Network, Key, Terminal, SlidersHorizontal, Eye, EyeOff, ChevronDown } from 'lucide-react';

const providers = ['OpenAI', 'Anthropic', 'OpenAI Compatible', 'Anthropic Compatible', 'Ollama', 'LM Studio'];

export default function AISettings() {
  const [activeProvider, setActiveProvider] = useState('OpenAI');
  const [showKey, setShowKey] = useState(false);
  const [temperature, setTemperature] = useState(0.7);
  const [maxTokens, setMaxTokens] = useState(4096);

  return (
    <div className="max-w-5xl mx-auto animate-in fade-in duration-500">
      {/* Provider Selection */}
      <section className="mb-12">
        <div className="flex items-center gap-2 mb-4">
          <Network className="text-primary" size={20} />
          <h3 className="text-sm font-bold uppercase tracking-widest text-on-surface-variant">Provider Selection</h3>
        </div>
        <div className="bg-surface-container-low p-1.5 rounded-xl flex flex-wrap gap-1">
          {providers.map(p => (
            <button
              key={p}
              onClick={() => setActiveProvider(p)}
              className={`px-5 py-2.5 text-sm font-semibold rounded-lg transition-all ${
                activeProvider === p
                  ? 'bg-surface-container-lowest text-primary shadow-sm ring-1 ring-outline-variant/20 scale-[0.98]'
                  : 'text-on-surface-variant hover:text-on-surface hover:bg-surface-container-lowest/50'
              }`}
            >
              {p}
            </button>
          ))}
        </div>
      </section>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-8 mb-12">
        {/* API Config */}
        <section className="space-y-6">
          <div className="flex items-center gap-2 mb-2">
            <Key className="text-primary" size={20} />
            <h3 className="text-sm font-bold uppercase tracking-widest text-on-surface-variant">API Config</h3>
          </div>
          <div className="space-y-4">
            <div className="group">
              <label className="block text-xs font-bold text-on-surface-variant mb-1.5 ml-1">Model Selection</label>
              <div className="relative">
                <select className="w-full px-4 py-3 bg-surface-container-lowest border border-outline-variant/20 rounded-xl text-sm text-on-surface focus:ring-2 focus:ring-primary/30 outline-none shadow-sm transition-all appearance-none cursor-pointer">
                  <option>gpt-4-turbo-preview</option>
                  <option>gpt-4o</option>
                  <option>gpt-3.5-turbo</option>
                </select>
                <ChevronDown className="absolute right-3 top-1/2 -translate-y-1/2 text-on-surface-variant pointer-events-none" size={18} />
              </div>
            </div>
            <div className="group">
              <label className="block text-xs font-bold text-on-surface-variant mb-1.5 ml-1">API Address</label>
              <input
                type="text"
                placeholder="https://api.openai.com/v1"
                className="w-full px-4 py-3 bg-surface-container-lowest border border-outline-variant/20 rounded-xl text-sm text-on-surface focus:ring-2 focus:ring-primary/30 outline-none shadow-sm transition-all"
              />
            </div>
            <div className="group">
              <label className="block text-xs font-bold text-on-surface-variant mb-1.5 ml-1">API Key</label>
              <div className="relative">
                <input
                  type={showKey ? "text" : "password"}
                  defaultValue="sk-proj-************************"
                  className="w-full px-4 py-3 bg-surface-container-lowest border border-outline-variant/20 rounded-xl text-sm text-on-surface focus:ring-2 focus:ring-primary/30 outline-none shadow-sm transition-all pr-10"
                />
                <button
                  onClick={() => setShowKey(!showKey)}
                  className="absolute right-3 top-1/2 -translate-y-1/2 text-on-surface-variant hover:text-primary transition-colors"
                >
                  {showKey ? <EyeOff size={18} /> : <Eye size={18} />}
                </button>
              </div>
            </div>
          </div>
        </section>

        {/* Default Prompt */}
        <section className="flex flex-col h-full">
          <div className="flex items-center gap-2 mb-4">
            <Terminal className="text-primary" size={20} />
            <h3 className="text-sm font-bold uppercase tracking-widest text-on-surface-variant">Default Prompt</h3>
          </div>
          <div className="flex-1 min-h-[220px]">
            <textarea
              className="w-full h-full p-4 bg-surface-container-lowest border border-outline-variant/20 rounded-xl text-sm text-on-surface focus:ring-2 focus:ring-primary/30 outline-none shadow-sm resize-none leading-relaxed"
              placeholder="Enter the base system instruction for the AI..."
            ></textarea>
          </div>
        </section>
      </div>

      {/* Advanced Parameters */}
      <section className="bg-surface-container-low/50 rounded-2xl p-8 border border-outline-variant/20 shadow-sm">
        <div className="flex items-center gap-2 mb-8">
          <SlidersHorizontal className="text-primary" size={20} />
          <h3 className="text-sm font-bold uppercase tracking-widest text-on-surface-variant">Advanced Parameters</h3>
        </div>
        <div className="grid grid-cols-1 md:grid-cols-2 gap-12">
          <div className="space-y-4">
            <div className="flex justify-between items-center mb-2">
              <label className="text-xs font-bold text-on-surface-variant uppercase tracking-wider">Max Tokens</label>
              <span className="px-3 py-1 bg-primary/10 text-primary text-xs font-black rounded-full">{maxTokens}</span>
            </div>
            <input
              type="number"
              value={maxTokens}
              onChange={(e) => setMaxTokens(Number(e.target.value))}
              className="w-full px-4 py-3 bg-surface-container-lowest border border-outline-variant/20 rounded-xl text-sm text-on-surface focus:ring-2 focus:ring-primary/30 outline-none shadow-sm transition-all"
            />
            <p className="text-[10px] text-on-surface-variant font-medium">Controls the maximum length of the generated response.</p>
          </div>
          <div className="space-y-4">
            <div className="flex justify-between items-center mb-2">
              <label className="text-xs font-bold text-on-surface-variant uppercase tracking-wider">Temperature</label>
              <span className="px-3 py-1 bg-primary/10 text-primary text-xs font-black rounded-full">{temperature.toFixed(1)}</span>
            </div>
            <div className="relative py-4">
              <input
                type="range"
                min="0"
                max="1"
                step="0.1"
                value={temperature}
                onChange={(e) => setTemperature(parseFloat(e.target.value))}
                className="w-full h-2 bg-surface-container-highest rounded-full appearance-none cursor-pointer accent-primary"
              />
              <div className="flex justify-between mt-3 px-1">
                <span className="text-[10px] font-bold text-on-surface-variant">0.0 (PRECISE)</span>
                <span className="text-[10px] font-bold text-on-surface-variant">1.0 (CREATIVE)</span>
              </div>
            </div>
          </div>
        </div>
      </section>

      {/* Action Bar */}
      <div className="mt-12 flex items-center justify-end pt-8 border-t border-outline-variant/20">
        <div className="flex gap-3">
          <button className="px-6 py-3 text-sm font-bold text-on-surface-variant hover:text-on-surface transition-colors">Discard</button>
          <button className="px-8 py-3 bg-primary text-white text-sm font-bold rounded-xl shadow-lg shadow-primary/20 hover:opacity-90 transition-all active:scale-[0.98]">
            Save Configuration
          </button>
        </div>
      </div>
    </div>
  );
}
