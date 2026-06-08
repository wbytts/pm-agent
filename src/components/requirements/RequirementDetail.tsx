import { Button, Tabs, Tag } from "antd";
import { CloseOutlined } from "@ant-design/icons";
import type { ReactNode } from "react";
import type { Requirement } from "../../data";
import { useWorkspaceStore } from "../../store";
import { priorityColor, statusMeta } from "./requirementMeta";

export function RequirementDetail({ requirement }: { requirement: Requirement | null }) {
  const closeRequirement = useWorkspaceStore((state) => state.closeRequirement);
  const detailTab = useWorkspaceStore((state) => state.detailTab);
  const setDetailTab = useWorkspaceStore((state) => state.setDetailTab);

  if (!requirement) {
    return null;
  }

  return (
    <aside className="desktop-inspector flex w-[400px] shrink-0 flex-col overflow-hidden border-l border-[#cfd4dc] bg-[#f7f8fa]">
      <div className="flex shrink-0 items-start justify-between gap-3 border-b border-[#d8dde5] px-4 py-3">
        <div className="min-w-0 flex-1">
          <div className="mb-0.5 text-[11px] text-[#667085]">
            {requirement.id} / {requirement.project}
          </div>
          <div className="text-[15px] font-bold leading-snug">{requirement.title}</div>
        </div>
        <Button type="text" size="small" icon={<CloseOutlined />} onClick={closeRequirement} />
      </div>
      <div className="flex shrink-0 flex-wrap gap-x-4 gap-y-2 border-b border-[#d8dde5] px-4 py-3 text-xs">
        <MetaItem label="优先级" value={<Tag color={priorityColor[requirement.priority]}>{requirement.priority}</Tag>} />
        <MetaItem label="类型" value={<Tag>{requirement.type}</Tag>} />
        <MetaItem label="版本" value={requirement.versionName} />
        <MetaItem label="状态" value={<Tag color={statusMeta[requirement.status].color}>{statusMeta[requirement.status].label}</Tag>} />
        <MetaItem label="负责人" value={requirement.assignee} />
        <MetaItem label="截止日期" value={requirement.dueDate} />
      </div>
      <Tabs
        activeKey={detailTab}
        onChange={setDetailTab}
        size="small"
        className="shrink-0 px-4"
        items={["描述", "子需求 (5)", "文档", "活动日志"].map((label) => ({ key: label, label }))}
      />
      <div className="flex-1 overflow-y-auto px-4 pb-4 text-[13px] leading-6">
        <h3 className="mb-1 mt-3 text-[13px] font-bold">描述</h3>
        <p>{requirement.description || "暂无描述"}</p>
        <h3 className="mb-1 mt-3 text-[13px] font-bold">验收标准</h3>
        <div className="rounded-md border border-[#d8dde5] bg-white p-2.5 text-xs">
          {(requirement.acceptanceCriteria.length > 0 ? requirement.acceptanceCriteria : ["暂无验收标准"]).map((item, index) => (
            <div key={item} className="py-1">
              {index === 0 ? "☑" : "☐"} {item}
            </div>
          ))}
        </div>
      </div>
    </aside>
  );
}

function MetaItem({ label, value }: { label: string; value: ReactNode }) {
  return (
    <div className="flex min-w-[72px] flex-col gap-0.5">
      <span className="text-[10px] font-semibold text-[#667085]">{label}</span>
      <span className="font-medium">{value}</span>
    </div>
  );
}
