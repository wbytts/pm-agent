import { ReloadOutlined, SendOutlined } from "@ant-design/icons";
import { Button, Input, Select } from "antd";
import { useEffect, useMemo, useState } from "react";
import {
  createAgentSession,
  listAgentModels,
  sendAgentPrompt,
  setAgentSessionModel,
  type AiModel,
  type PmAgentSession,
} from "./agentClient";

export function AgentPanel() {
  const [session, setSession] = useState<PmAgentSession | null>(null);
  const [prompt, setPrompt] = useState("");
  const [models, setModels] = useState<AiModel[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const messages = useMemo(() => session?.messages ?? [], [session]);

  useEffect(() => {
    void loadModels();
    void resetSession();
  }, []);

  async function loadModels() {
    try {
      setModels(await listAgentModels());
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error));
    }
  }

  async function resetSession() {
    setLoading(true);
    setError(null);
    try {
      const nextSession = await createAgentSession();
      setSession(nextSession);
      setPrompt("");
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error));
    } finally {
      setLoading(false);
    }
  }

  async function sendPrompt() {
    const content = prompt.trim();
    if (!session || !content) {
      return;
    }

    setLoading(true);
    setError(null);
    try {
      const response = await sendAgentPrompt(session, content);
      setSession(response.session);
      setPrompt("");
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error));
    } finally {
      setLoading(false);
    }
  }

  async function changeModel(value: string) {
    if (!session) {
      return;
    }
    const model = models.find((item) => modelKey(item) === value);
    if (!model) {
      return;
    }
    setLoading(true);
    setError(null);
    try {
      setSession(await setAgentSessionModel(session, model));
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error));
    } finally {
      setLoading(false);
    }
  }

  return (
    <aside className="agent-panel flex w-[320px] shrink-0 flex-col border-l border-[#cfd4dc] bg-[#f7f8fa]">
      <div className="flex h-[40px] shrink-0 items-center justify-between border-b border-[#d8dde5] px-3">
        <div className="text-[13px] font-semibold">Agent</div>
        <div className="flex items-center gap-1.5">
          <Select
            size="small"
            className="w-[132px]"
            value={session ? modelKey(session.model) : undefined}
            options={models.map((model) => ({
              value: modelKey(model),
              label: model.displayName,
            }))}
            onChange={changeModel}
          />
          <Button type="text" size="small" icon={<ReloadOutlined />} onClick={resetSession} />
        </div>
      </div>
      <div className="min-h-0 flex-1 space-y-2 overflow-y-auto px-3 py-3">
        {messages.length === 0 && (
          <div className="rounded-md border border-dashed border-[#c3cad5] bg-white px-3 py-3 text-xs leading-5 text-[#667085]">
            右侧面板已连接 Rust `pm-agent` 子包，可以在这里和 agent 交互。
          </div>
        )}
        {messages.map((message, index) => (
          <div
            key={`${message.role}-${index}`}
            className={`rounded-md border px-3 py-2 text-xs leading-5 ${
              message.role === "User"
                ? "border-[#bfdbfe] bg-[#eff6ff] text-[#1e3a8a]"
                : "border-[#d8dde5] bg-white text-[#1f2328]"
            }`}
          >
            <div className="mb-1 text-[10px] font-semibold text-[#667085]">{message.role === "User" ? "你" : "Agent"}</div>
            <div className="whitespace-pre-wrap">{message.content}</div>
          </div>
        ))}
        {error && <div className="rounded-md border border-[#fecaca] bg-[#fff1f0] px-3 py-2 text-xs text-[#b42318]">{error}</div>}
      </div>
      <div className="shrink-0 border-t border-[#d8dde5] bg-[#fbfbfc] p-3">
        <Input.TextArea
          value={prompt}
          onChange={(event) => setPrompt(event.target.value)}
          rows={3}
          placeholder="输入要交给 agent 的任务"
          onPressEnter={(event) => {
            if (event.shiftKey) {
              return;
            }
            event.preventDefault();
            void sendPrompt();
          }}
        />
        <div className="mt-2 flex justify-end">
          <Button type="primary" size="small" icon={<SendOutlined />} loading={loading} onClick={sendPrompt}>
            发送
          </Button>
        </div>
      </div>
    </aside>
  );
}

function modelKey(model: AiModel) {
  return `${model.provider}/${model.id}`;
}
