import { Card } from "antd";
import type { Project } from "../../data";
import { useWorkspaceStore } from "../../store";

export function ProjectCard({
  project,
  versions = [],
  onCreateVersion,
}: {
  project: Project;
  versions?: Array<{ id: string; name: string; requirementCount: number }>;
  onCreateVersion?: (project: Project) => void;
}) {
  const openProjectRequirements = useWorkspaceStore((state) => state.openProjectRequirements);

  return (
    <Card
      size="small"
      hoverable
      className="desktop-card project-card h-full"
      onClick={() => openProjectRequirements(project.id)}
    >
      <div className="mb-3 flex items-start justify-between gap-3">
        <div className="min-w-0 flex-1">
          <div className="truncate text-[15px] font-bold">{project.name}</div>
          <div className="mt-1 text-[11px] text-[#667085]">{project.owner || "未设置负责人"}</div>
        </div>
      </div>
      <div className="line-clamp-2 min-h-[36px] text-xs leading-[18px] text-[#667085]">
        {project.description || "暂无项目描述"}
      </div>
      <div className="mt-4 flex flex-wrap gap-1.5">
        {versions.map((version) => (
          <button
            key={version.id}
            className="rounded-md border border-[#d8dde5] bg-[#fbfbfc] px-2 py-1 text-[11px] text-[#475467] hover:border-[#2563eb] hover:text-[#1d4ed8]"
            onClick={(event) => {
              event.stopPropagation();
              openProjectRequirements(project.id, version.id);
            }}
          >
            {version.name}
          </button>
        ))}
        {onCreateVersion && (
          <button
            className="rounded-md border border-dashed border-[#c3cad5] bg-transparent px-2 py-1 text-[11px] text-[#667085] hover:border-[#2563eb] hover:text-[#1d4ed8]"
            onClick={(event) => {
              event.stopPropagation();
              onCreateVersion(project);
            }}
          >
            新建版本
          </button>
        )}
      </div>
    </Card>
  );
}
