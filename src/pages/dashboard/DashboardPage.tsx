import { Card } from "antd";
import { SectionHeader } from "../../components/common/SectionHeader";
import { ProjectCard } from "../../components/projects/ProjectCard";
import { useWorkspaceStore } from "../../store";

export function DashboardPage() {
  const projects = useWorkspaceStore((state) => state.projects);
  const requirements = useWorkspaceStore((state) => state.requirements);
  const activeProjects = projects.filter((project) => project.status === "active").length;
  const todoRequirements = requirements.filter((requirement) => requirement.status !== "done").length;
  const p0Requirements = requirements.filter((requirement) => requirement.priority === "P0" && requirement.status !== "done").length;
  const doneRequirements = requirements.filter((requirement) => requirement.status === "done").length;

  return (
    <div className="h-full overflow-y-auto p-4">
      <div className="mb-4 grid grid-cols-4 gap-2.5">
        <StatCard label="活跃项目" value={String(activeProjects)} change={`共 ${projects.length} 个项目`} />
        <StatCard label="待办需求" value={String(todoRequirements)} change={`其中 P0 级别 ${p0Requirements} 个`} warning />
        <StatCard label="知识条目" value="36" change="本周新增 5 条" />
        <StatCard label="已完成需求" value={String(doneRequirements)} change="来自本地数据库" />
      </div>
      <SectionHeader title="活跃项目" action="查看全部" view="projects" />
      <div className="space-y-2.5">
        {projects.slice(0, 2).map((project) => (
          <ProjectCard key={project.id} project={project} />
        ))}
      </div>
    </div>
  );
}

function StatCard({ label, value, change, warning = false }: { label: string; value: string; change: string; warning?: boolean }) {
  return (
    <Card size="small" className="desktop-card">
      <div className="mb-1 text-[11px] font-semibold text-[#667085]">{label}</div>
      <div className="text-[24px] font-semibold leading-tight">{value}</div>
      <div className={`mt-0.5 text-[11px] ${warning ? "text-[#b45309]" : "text-[#15803d]"}`}>{change}</div>
    </Card>
  );
}
