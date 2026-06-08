import { Card, Tabs } from "antd";
import { knowledgeItems } from "../../data";
import { useWorkspaceStore } from "../../store";

export function KnowledgePage() {
  const knowledgeTab = useWorkspaceStore((state) => state.knowledgeTab);
  const setKnowledgeTab = useWorkspaceStore((state) => state.setKnowledgeTab);

  return (
    <div className="h-full overflow-y-auto p-4">
      <Tabs
        activeKey={knowledgeTab}
        onChange={setKnowledgeTab}
        items={[
          { key: "project", label: "项目知识库" },
          { key: "personal", label: "个人知识库" },
          { key: "general", label: "通用知识库" },
        ]}
      />
      <div className="grid grid-cols-[repeat(auto-fill,minmax(280px,1fr))] gap-2.5">
        {knowledgeItems.map((item) => (
          <Card key={item.title} size="small" hoverable className="desktop-card">
            <div className="mb-1 text-[10px] text-[#667085]">{item.scope}</div>
            <div className="mb-1 text-[13px] font-semibold">{item.title}</div>
            <div className="line-clamp-2 text-[11px] text-[#667085]">{item.description}</div>
            <div className="mt-2 flex justify-between text-[10px] text-[#667085]">
              <span>{item.footer}</span>
              <span>{item.metric}</span>
            </div>
          </Card>
        ))}
      </div>
    </div>
  );
}
