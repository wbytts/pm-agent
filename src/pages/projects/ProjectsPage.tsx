import { Button } from "antd";
import { FolderAddOutlined } from "@ant-design/icons";
import { ProjectCard } from "../../components/projects/ProjectCard";
import type { Project } from "../../data";
import { useWorkspaceStore } from "../../store";

export function ProjectsPage({
  onCreateProject,
  onCreateVersion,
}: {
  onCreateProject: () => void;
  onCreateVersion: (project: Project) => void;
}) {
  const projects = useWorkspaceStore((state) => state.projects);
  const projectVersions = useWorkspaceStore((state) => state.projectVersions);
  const loading = useWorkspaceStore((state) => state.loading);

  return (
    <div className="h-full overflow-y-auto p-4">
      <div className="mb-3 flex items-center justify-between">
        <h2 className="m-0 text-sm font-bold">我的项目</h2>
        <Button size="small" type="primary" icon={<FolderAddOutlined />} onClick={onCreateProject}>
          新建项目
        </Button>
      </div>
      <div className="grid grid-cols-[repeat(auto-fill,minmax(260px,1fr))] gap-3">
        {loading && <div className="text-xs text-[#667085]">正在加载项目...</div>}
        {!loading && projects.length === 0 && <div className="text-xs text-[#667085]">暂无项目</div>}
        {projects.map((project) => (
          <ProjectCard
            key={project.id}
            project={project}
            versions={projectVersions.filter((version) => version.projectId === project.id)}
            onCreateVersion={onCreateVersion}
          />
        ))}
      </div>
    </div>
  );
}
