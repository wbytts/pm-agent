import {
  Button,
  ConfigProvider,
  Form,
  Input,
  Modal,
  Select,
  theme,
} from "antd";
import zhCN from "antd/locale/zh_CN";
import {
  MenuFoldOutlined,
  SearchOutlined,
} from "@ant-design/icons";
import { useEffect, useState } from "react";
import { DashboardPage, GeneratePage, KnowledgePage, ProjectsPage, RequirementsPage, SettingsPage } from "./pages";
import { AgentPanel } from "./windows";
import {
  knowledgeItems,
  navItems,
  viewTitles,
  type Project,
  type ViewKey,
} from "./data";
import { useWorkspaceStore } from "./store";

export function WorkspaceApp() {
  return (
    <ConfigProvider
      locale={zhCN}
      theme={{
        algorithm: theme.defaultAlgorithm,
        token: {
          colorPrimary: "#2563eb",
          borderRadius: 5,
          colorBgLayout: "#eef0f3",
          fontFamily:
            "Plus Jakarta Sans, -apple-system, BlinkMacSystemFont, Segoe UI, system-ui, sans-serif",
        },
        components: {
          Button: { controlHeightSM: 28 },
          Card: { borderRadiusLG: 6 },
          Table: { cellPaddingBlock: 8, cellPaddingInline: 12 },
        },
      }}
    >
      <WorkspaceShell />
    </ConfigProvider>
  );
}

function WorkspaceShell() {
  const activeView = useWorkspaceStore((state) => state.activeView);
  const setActiveView = useWorkspaceStore((state) => state.setActiveView);
  const loadWorkspace = useWorkspaceStore((state) => state.loadWorkspace);
  const projects = useWorkspaceStore((state) => state.projects);
  const requirements = useWorkspaceStore((state) => state.requirements);
  const databaseError = useWorkspaceStore((state) => state.databaseError);
  const [requirementModalOpen, setRequirementModalOpen] = useState(false);
  const [projectModalOpen, setProjectModalOpen] = useState(false);
  const [versionModalProject, setVersionModalProject] = useState<Project | null>(null);

  useEffect(() => {
    void loadWorkspace();
  }, [loadWorkspace]);

  return (
    <div className="desktop-window flex h-full flex-col bg-[#eef0f3] text-[#1f2328]">
      <header className="desktop-toolbar flex h-[46px] shrink-0 items-center gap-3 border-b border-[#cfd4dc] bg-[#f6f7f9]/95 px-3">
        <Button type="text" size="small" icon={<MenuFoldOutlined />} />
        <div className="toolbar-title min-w-[150px] text-[13px] font-semibold">产品工作台</div>
        <div className="toolbar-search flex min-w-[240px] max-w-[420px] flex-1 items-center gap-2 rounded-md border border-[#d6dae1] bg-white px-2 py-1 text-xs text-[#667085]">
          <SearchOutlined />
          <span>搜索项目、需求、知识条目</span>
        </div>
      </header>

      <div className="flex min-h-0 flex-1">
        <aside className="desktop-sidebar flex w-[196px] shrink-0 select-none flex-col border-r border-[#cfd4dc] bg-[#e8ebf0]">
          <nav className="flex flex-1 flex-col gap-0.5 overflow-y-auto p-2">
            {renderNavItems(activeView, setActiveView, projects.length, requirements.length)}
          </nav>
          <div className="border-t border-[#cfd4dc] p-2">
            <div className="flex cursor-pointer items-center gap-2 rounded-md px-2 py-1.5 hover:bg-[#dfe3ea]">
              <div className="flex h-6 w-6 items-center justify-center rounded-md bg-[#2563eb] text-[11px] font-semibold text-white">
                张
              </div>
              <div className="min-w-0 flex-1">
                <div className="text-[12px] font-semibold">张明远</div>
                <div className="truncate text-[11px] text-[#667085]">高级产品经理</div>
              </div>
            </div>
          </div>
        </aside>

        <main className="flex min-w-0 flex-1 flex-col">
          <div className="content-titlebar flex h-[40px] shrink-0 items-center gap-3 border-b border-[#d8dde5] bg-[#fbfbfc] px-4">
            <h1 className="m-0 text-[13px] font-semibold">{viewTitles[activeView]}</h1>
            <span className="text-[11px] text-[#667085]">{getViewSubtitle(activeView, projects.length, requirements.length)}</span>
          </div>

          {databaseError && (
            <div className="border-b border-[#f4c7c7] bg-[#fff1f0] px-4 py-2 text-xs text-[#b42318]">
              数据库连接失败：{databaseError}
            </div>
          )}

          <section className="min-h-0 flex-1 overflow-hidden">
            {activeView === "dashboard" && <DashboardPage />}
            {activeView === "projects" && (
              <ProjectsPage
                onCreateProject={() => setProjectModalOpen(true)}
                onCreateVersion={setVersionModalProject}
              />
            )}
            {activeView === "requirements" && (
              <RequirementsPage onCreateRequirement={() => setRequirementModalOpen(true)} />
            )}
            {activeView === "knowledge" && <KnowledgePage />}
            {activeView === "generate" && <GeneratePage />}
            {activeView === "settings" && <SettingsPage />}
          </section>
        </main>

        <AgentPanel />
      </div>

      <RequirementModal open={requirementModalOpen} onClose={() => setRequirementModalOpen(false)} />
      <ProjectModal open={projectModalOpen} onClose={() => setProjectModalOpen(false)} />
      <VersionModal project={versionModalProject} onClose={() => setVersionModalProject(null)} />
    </div>
  );
}

function getViewSubtitle(view: ViewKey, projectCount: number, requirementCount: number) {
  const subtitles: Record<ViewKey, string> = {
    dashboard: "今日概览与高优先级事项",
    projects: `${projectCount} 个项目`,
    requirements: `${requirementCount} 条需求`,
    knowledge: `${knowledgeItems.length} 条知识`,
    generate: "基于需求和知识库产出文档",
    settings: "个人偏好与生成配置",
  };

  return subtitles[view];
}

function renderNavItems(
  activeView: ViewKey,
  setActiveView: (view: ViewKey) => void,
  projectCount: number,
  requirementCount: number,
) {
  let lastSection = "";

  return navItems.flatMap((item) => {
    const badge = item.key === "projects" ? String(projectCount) : item.key === "requirements" ? String(requirementCount) : item.badge;
    const nodes = [];
    if (item.section !== lastSection) {
      lastSection = item.section;
      nodes.push(
        <div key={`${item.section}-section`} className="px-2 pb-1 pt-3 text-[10px] font-semibold text-[#667085]">
          {item.section}
        </div>,
      );
    }

    nodes.push(
      <button
        key={item.key}
        className={`flex w-full cursor-pointer items-center gap-2.5 rounded-md border-0 px-3 py-2 text-left text-[13px] font-medium transition ${
          activeView === item.key
            ? "bg-[#d9e7ff] text-[#1d4ed8] shadow-[inset_0_0_0_1px_rgba(37,99,235,0.16)]"
            : "bg-transparent text-[#475467] hover:bg-[#dfe3ea] hover:text-[#1f2328]"
        }`}
        onClick={() => setActiveView(item.key)}
      >
        <span className="text-[15px] leading-none">{item.icon}</span>
        <span>{item.label}</span>
        {badge && (
          <span
            className={`ml-auto rounded-full px-2 py-0.5 text-[11px] ${
              item.primaryBadge ? "bg-[#2563eb] text-white" : "bg-[#d5dae2] text-[#475467]"
            }`}
          >
            {badge}
          </span>
        )}
      </button>,
    );

    return nodes;
  });
}

function RequirementModal({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [form] = Form.useForm();
  const projects = useWorkspaceStore((state) => state.projects);
  const projectVersions = useWorkspaceStore((state) => state.projectVersions);
  const createRequirement = useWorkspaceStore((state) => state.createRequirement);
  const [confirmLoading, setConfirmLoading] = useState(false);
  const selectedProjectId = Form.useWatch("projectId", form) as string | undefined;
  const versionOptions = projectVersions
    .filter((version) => version.projectId === selectedProjectId)
    .map((version) => ({ value: version.id, label: version.name }));

  useEffect(() => {
    if (open && projects[0] && !form.getFieldValue("projectId")) {
      form.setFieldsValue({ projectId: projects[0].id });
    }
  }, [form, open, projects]);

  useEffect(() => {
    if (!open || !selectedProjectId) {
      return;
    }
    const currentVersionId = form.getFieldValue("versionId");
    const firstVersion = projectVersions.find((version) => version.projectId === selectedProjectId);
    const currentVersionAvailable = projectVersions.some(
      (version) => version.projectId === selectedProjectId && version.id === currentVersionId,
    );
    if (firstVersion && !currentVersionAvailable) {
      form.setFieldsValue({ versionId: firstVersion.id });
    }
  }, [form, open, projectVersions, selectedProjectId]);

  async function handleOk() {
    const values = await form.validateFields();
    setConfirmLoading(true);
    try {
      await createRequirement({
        title: values.title,
        projectId: values.projectId,
        versionId: values.versionId,
        type: values.type,
        priority: values.priority,
        assignee: values.assignee || "-",
        dueDate: values.dueDate || "",
        description: values.description || "",
      });
      form.resetFields();
      onClose();
    } finally {
      setConfirmLoading(false);
    }
  }

  return (
    <Modal
      className="pm-modal"
      title="新建需求"
      open={open}
      onCancel={onClose}
      onOk={handleOk}
      okText="创建"
      cancelText="取消"
      confirmLoading={confirmLoading}
      okButtonProps={{ disabled: projects.length === 0 || versionOptions.length === 0 }}
    >
      <Form
        form={form}
        layout="vertical"
        initialValues={{ projectId: projects[0]?.id, versionId: versionOptions[0]?.value, type: "Story", priority: "P1" }}
      >
        <Form.Item label="标题" name="title" rules={[{ required: true, message: "请输入需求标题" }]}>
          <Input placeholder="例如：工单批量操作功能" />
        </Form.Item>
        <Form.Item label="所属项目" name="projectId" rules={[{ required: true, message: "请选择所属项目" }]}>
          <Select options={projects.map((item) => ({ value: item.id, label: item.name }))} />
        </Form.Item>
        <Form.Item label="所属版本" name="versionId" rules={[{ required: true, message: "请选择所属版本" }]}>
          <Select options={versionOptions} />
        </Form.Item>
        <Form.Item label="类型" name="type">
          <Select options={["Epic", "Story", "Task"].map((value) => ({ value }))} />
        </Form.Item>
        <Form.Item label="优先级" name="priority">
          <Select
            options={[
              { value: "P0", label: "P0 - 紧急" },
              { value: "P1", label: "P1 - 重要" },
              { value: "P2", label: "P2 - 一般" },
            ]}
          />
        </Form.Item>
        <Form.Item label="负责人" name="assignee">
          <Input placeholder="选择或输入负责人" />
        </Form.Item>
        <Form.Item label="截止日期" name="dueDate">
          <Input type="date" />
        </Form.Item>
        <Form.Item label="描述 / 验收标准" name="description">
          <Input.TextArea rows={4} placeholder="第一行作为描述，也可逐行填写验收标准" />
        </Form.Item>
      </Form>
    </Modal>
  );
}

function ProjectModal({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [form] = Form.useForm();
  const createProject = useWorkspaceStore((state) => state.createProject);
  const [confirmLoading, setConfirmLoading] = useState(false);

  async function handleOk() {
    const values = await form.validateFields();
    setConfirmLoading(true);
    try {
      await createProject({
        name: values.name,
        description: values.description || "",
        dueDate: "",
        owner: values.owner || "张明远",
        members: 1,
      });
      form.resetFields();
      onClose();
    } finally {
      setConfirmLoading(false);
    }
  }

  return (
    <Modal
      className="pm-modal"
      title="新建项目"
      open={open}
      onCancel={onClose}
      onOk={handleOk}
      okText="创建"
      cancelText="取消"
      confirmLoading={confirmLoading}
    >
      <Form form={form} layout="vertical" initialValues={{ owner: "张明远" }}>
        <Form.Item label="项目名称" name="name" rules={[{ required: true, message: "请输入项目名称" }]}>
          <Input placeholder="例如：用户反馈系统 V2" />
        </Form.Item>
        <Form.Item label="项目简述" name="description">
          <Input.TextArea rows={2} placeholder="一句话描述项目目标和范围" />
        </Form.Item>
        <Form.Item label="负责人" name="owner">
          <Input placeholder="输入负责人名称" />
        </Form.Item>
      </Form>
    </Modal>
  );
}

function VersionModal({ project, onClose }: { project: Project | null; onClose: () => void }) {
  const [form] = Form.useForm();
  const createProjectVersion = useWorkspaceStore((state) => state.createProjectVersion);
  const [confirmLoading, setConfirmLoading] = useState(false);
  const open = Boolean(project);

  async function handleOk() {
    if (!project) {
      return;
    }
    const values = await form.validateFields();
    setConfirmLoading(true);
    try {
      await createProjectVersion({
        projectId: project.id,
        name: values.name,
        description: values.description || "",
      });
      form.resetFields();
      onClose();
    } finally {
      setConfirmLoading(false);
    }
  }

  return (
    <Modal
      className="pm-modal"
      title={project ? `新建版本 / ${project.name}` : "新建版本"}
      open={open}
      onCancel={onClose}
      onOk={handleOk}
      okText="创建"
      cancelText="取消"
      confirmLoading={confirmLoading}
    >
      <Form form={form} layout="vertical">
        <Form.Item label="版本名称" name="name" rules={[{ required: true, message: "请输入版本名称" }]}>
          <Input placeholder="例如：v1.0、2026 Q1、基础版" />
        </Form.Item>
        <Form.Item label="版本说明" name="description">
          <Input.TextArea rows={3} placeholder="描述这个版本的范围或目标" />
        </Form.Item>
      </Form>
    </Modal>
  );
}
