import { FileAddOutlined, MoreOutlined } from "@ant-design/icons";
import { Button, Checkbox, Input, Select, Table, Tag } from "antd";
import type { ColumnsType } from "antd/es/table";
import { useMemo } from "react";
import { RequirementDetail } from "../../components/requirements/RequirementDetail";
import { priorityColor, statusMeta } from "../../components/requirements/requirementMeta";
import type { Requirement, RequirementPriority, RequirementStatus } from "../../data";
import { useWorkspaceStore } from "../../store";

export function RequirementsPage({ onCreateRequirement }: { onCreateRequirement: () => void }) {
  const projects = useWorkspaceStore((state) => state.projects);
  const projectVersions = useWorkspaceStore((state) => state.projectVersions);
  const requirements = useWorkspaceStore((state) => state.requirements);
  const selectedRequirementId = useWorkspaceStore((state) => state.selectedRequirementId);
  const requirementSearch = useWorkspaceStore((state) => state.requirementSearch);
  const projectFilter = useWorkspaceStore((state) => state.projectFilter);
  const versionFilter = useWorkspaceStore((state) => state.versionFilter);
  const priorityFilter = useWorkspaceStore((state) => state.priorityFilter);
  const statusFilter = useWorkspaceStore((state) => state.statusFilter);
  const openRequirement = useWorkspaceStore((state) => state.openRequirement);
  const setRequirementSearch = useWorkspaceStore((state) => state.setRequirementSearch);
  const setProjectFilter = useWorkspaceStore((state) => state.setProjectFilter);
  const setVersionFilter = useWorkspaceStore((state) => state.setVersionFilter);
  const setPriorityFilter = useWorkspaceStore((state) => state.setPriorityFilter);
  const setStatusFilter = useWorkspaceStore((state) => state.setStatusFilter);
  const selectedRequirement = requirements.find((item) => item.id === selectedRequirementId) ?? null;
  const filteredRequirements = useMemo(
    () =>
      requirements.filter((requirement) => {
        const keyword = requirementSearch.trim().toLowerCase();
        const matchedKeyword =
          keyword.length === 0 ||
          requirement.id.toLowerCase().includes(keyword) ||
          requirement.title.toLowerCase().includes(keyword) ||
          requirement.description.toLowerCase().includes(keyword);
        const matchedProject = projectFilter === "all" || requirement.projectId === projectFilter;
        const matchedVersion = versionFilter === "all" || requirement.versionId === versionFilter;
        const matchedPriority = priorityFilter === "all" || requirement.priority === priorityFilter;
        const matchedStatus = statusFilter === "all" || requirement.status === statusFilter;

        return matchedKeyword && matchedProject && matchedVersion && matchedPriority && matchedStatus;
      }),
    [projectFilter, requirements, requirementSearch, priorityFilter, statusFilter, versionFilter],
  );
  const versionOptions = useMemo(
    () => [
      { value: "all", label: "全部版本" },
      ...projectVersions
        .filter((version) => projectFilter === "all" || version.projectId === projectFilter)
        .map((version) => ({ value: version.id, label: version.name })),
    ],
    [projectFilter, projectVersions],
  );

  const columns = useMemo<ColumnsType<Requirement>>(
    () => [
      {
        title: <Checkbox />,
        dataIndex: "checked",
        width: 44,
        render: () => <Checkbox onClick={(event) => event.stopPropagation()} />,
      },
      {
        title: "ID",
        dataIndex: "id",
        width: 82,
        render: (value: string) => <span className="text-[11px] text-[#667085]">{value}</span>,
      },
      {
        title: "需求标题",
        dataIndex: "title",
        ellipsis: true,
        render: (value: string) => <span className="font-medium">{value}</span>,
      },
      {
        title: "优先级",
        dataIndex: "priority",
        width: 90,
        render: (value: RequirementPriority) => <Tag color={priorityColor[value]}>{value}</Tag>,
      },
      {
        title: "类型",
        dataIndex: "type",
        width: 88,
        render: (value: string) => <Tag>{value}</Tag>,
      },
      {
        title: "版本",
        dataIndex: "versionName",
        width: 120,
        ellipsis: true,
      },
      {
        title: "状态",
        dataIndex: "status",
        width: 96,
        render: (value: RequirementStatus) => <Tag color={statusMeta[value].color}>{statusMeta[value].label}</Tag>,
      },
      { title: "负责人", dataIndex: "assignee", width: 90 },
      { title: "截止日期", dataIndex: "dueDate", width: 110 },
      {
        title: "",
        dataIndex: "actions",
        width: 56,
        render: () => (
          <Button type="text" size="small" icon={<MoreOutlined />} onClick={(event) => event.stopPropagation()} />
        ),
      },
    ],
    [],
  );

  return (
    <div className="flex h-full min-h-0">
      <div className="flex min-w-0 flex-1 flex-col">
        <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-[#d8dde5] bg-[#fbfbfc] px-4 py-2.5">
          <Input.Search
            className="min-w-[220px] flex-1"
            placeholder="搜索需求标题、ID 或描述"
            allowClear
            value={requirementSearch}
            onChange={(event) => setRequirementSearch(event.target.value)}
          />
          <Select
            className="w-44"
            value={projectFilter}
            onChange={setProjectFilter}
            options={[{ value: "all", label: "全部项目" }, ...projects.map((item) => ({ value: item.id, label: item.name }))]}
          />
          <Select className="w-36" value={versionFilter} onChange={setVersionFilter} options={versionOptions} />
          <Select
            className="w-36"
            value={priorityFilter}
            onChange={setPriorityFilter}
            options={[
              { value: "all", label: "全部优先级" },
              { value: "P0", label: "P0 - 紧急" },
              { value: "P1", label: "P1 - 重要" },
              { value: "P2", label: "P2 - 一般" },
            ]}
          />
          <Select
            className="w-32"
            value={statusFilter}
            onChange={setStatusFilter}
            options={[
              { value: "all", label: "全部状态" },
              { value: "todo", label: "待开始" },
              { value: "doing", label: "进行中" },
              { value: "review", label: "待评审" },
              { value: "done", label: "已完成" },
            ]}
          />
          <Button type="primary" size="small" icon={<FileAddOutlined />} onClick={onCreateRequirement}>
            新建
          </Button>
        </div>
        <div className="min-h-0 flex-1 overflow-auto">
          <Table<Requirement>
            className="pm-table"
            rowKey="id"
            columns={columns}
            dataSource={filteredRequirements}
            pagination={false}
            size="middle"
            sticky
            onRow={(record) => ({
              onClick: () => openRequirement(record.id),
              className: record.id === selectedRequirementId ? "selected-row" : "",
            })}
          />
        </div>
        <div className="flex shrink-0 items-center justify-between border-t border-[#d8dde5] bg-[#fbfbfc] px-4 py-2 text-xs text-[#667085]">
          <span>共 {filteredRequirements.length} 条需求 / 已选 0 项</span>
          <div className="flex items-center gap-1">
            <span>每页 20 条</span>
            <Button size="small" disabled>
              ‹
            </Button>
            <Button size="small" type="primary">
              1
            </Button>
            <Button size="small">›</Button>
          </div>
        </div>
      </div>
      <RequirementDetail requirement={selectedRequirement} />
    </div>
  );
}
