import type { ReactNode } from "react";
import {
  AppstoreOutlined,
  BookOutlined,
  CheckSquareOutlined,
  FileTextOutlined,
  FolderOpenOutlined,
  SettingOutlined,
} from "@ant-design/icons";

export type ViewKey =
  | "dashboard"
  | "projects"
  | "requirements"
  | "knowledge"
  | "generate"
  | "settings";

export type RequirementStatus = "todo" | "doing" | "review" | "done";
export type RequirementPriority = "P0" | "P1" | "P2";

export interface Project {
  id: string;
  name: string;
  description: string;
  status: "active" | "planning";
  owner: string;
  dueDate: string;
  members: number;
  totalRequirements: number;
  completedRequirements: number;
  progress: number;
}

export interface ProjectVersion {
  id: string;
  projectId: string;
  name: string;
  description: string;
  status: "active" | "archived";
  requirementCount: number;
}

export interface Requirement {
  id: string;
  title: string;
  projectId: string;
  project: string;
  versionId: string;
  versionName: string;
  priority: RequirementPriority;
  type: "Epic" | "Story" | "Task";
  status: RequirementStatus;
  assignee: string;
  dueDate: string;
  description: string;
  acceptanceCriteria: string[];
}

export interface KnowledgeItem {
  scope: string;
  title: string;
  description: string;
  footer: string;
  metric: string;
}

export const viewTitles: Record<ViewKey, string> = {
  dashboard: "工作台",
  projects: "项目",
  requirements: "需求",
  knowledge: "知识库",
  generate: "生成文档",
  settings: "设置",
};

export const navItems: Array<{
  key: ViewKey;
  label: string;
  section: string;
  icon: ReactNode;
  badge?: string;
  primaryBadge?: boolean;
}> = [
  { key: "dashboard", label: "工作台", section: "工作区", icon: <AppstoreOutlined /> },
  {
    key: "projects",
    label: "项目",
    section: "工作区",
    icon: <FolderOpenOutlined />,
    badge: "4",
    primaryBadge: true,
  },
  {
    key: "requirements",
    label: "需求",
    section: "工作区",
    icon: <CheckSquareOutlined />,
    badge: "24",
  },
  { key: "knowledge", label: "知识库", section: "知识", icon: <BookOutlined /> },
  { key: "generate", label: "生成文档", section: "产出", icon: <FileTextOutlined /> },
  { key: "settings", label: "设置", section: "系统", icon: <SettingOutlined /> },
];

export const projects: Project[] = [
  {
    id: "cs-2",
    name: "客服平台 2.0 重构",
    description: "全渠道客服工作台重构，统一工单、在线对话、电话回拨三大通道，支持智能分配与 SLA 管理",
    status: "active",
    owner: "张明远",
    dueDate: "2025-06-30",
    members: 5,
    totalRequirements: 12,
    completedRequirements: 4,
    progress: 65,
  },
  {
    id: "growth-mobile",
    name: "移动端用户增长系统",
    description: "搭建用户增长引擎：邀请裂变、签到激励、个性化推送",
    status: "planning",
    owner: "李安",
    dueDate: "2025-07-15",
    members: 3,
    totalRequirements: 8,
    completedRequirements: 0,
    progress: 20,
  },
  {
    id: "analytics-v3",
    name: "数据分析看板 V3",
    description: "实时数据看板重构：客服并发、工单趋势、满意度排行、导出报表",
    status: "active",
    owner: "王晨",
    dueDate: "2025-05-20",
    members: 4,
    totalRequirements: 6,
    completedRequirements: 4,
    progress: 72,
  },
];

export const requirements: Requirement[] = [
  [
    "REQ-101",
    "统一工单中心 - 多通道工单聚合展示",
    "cs-2",
    "客服平台 2.0 重构",
    "P0",
    "Epic",
    "doing",
    "李安",
    "2025-05-20",
    "将所有渠道产生的客户请求统一转化为工单，在一个工作台内集中处理。",
    ["100 条工单列表加载时间 <= 1s", "支持实时新工单推送", "工单状态变更记录完整可追溯"],
  ],
  [
    "REQ-102",
    "智能路由分配 - 基于技能组的工作量均衡",
    "cs-2",
    "客服平台 2.0 重构",
    "P0",
    "Epic",
    "todo",
    "赵磊",
    "2025-06-05",
    "按照客服技能组、当前负载和 SLA 风险自动分配新工单。",
    ["支持技能组匹配", "支持负载均衡", "分配结果可追溯"],
  ],
  [
    "REQ-103",
    "SLA 看板 - 实时超时预警与降级策略",
    "cs-2",
    "客服平台 2.0 重构",
    "P1",
    "Story",
    "review",
    "王晨",
    "2025-06-15",
    "集中展示即将超时和已超时工单，并支持负责人快速处理。",
    ["按优先级展示 SLA 风险", "支持超时原因记录", "支持导出看板数据"],
  ],
  [
    "REQ-104",
    "客户侧聊天组件重构 - Web SDK 3.0",
    "cs-2",
    "客服平台 2.0 重构",
    "P1",
    "Epic",
    "todo",
    "陈思",
    "2025-06-20",
    "重构客户侧聊天 SDK，改善加载性能、事件追踪和多渠道兼容。",
    ["首屏加载体积下降", "事件埋点完整", "兼容主流浏览器"],
  ],
  [
    "REQ-105",
    "历史工单导入工具 - CSV/API 批量导入",
    "cs-2",
    "客服平台 2.0 重构",
    "P2",
    "Task",
    "todo",
    "-",
    "2025-06-25",
    "支持历史工单通过 CSV 或 API 批量导入，并保留来源信息。",
    ["支持 CSV 导入", "导入失败可重试", "保留原始渠道来源"],
  ],
  [
    "REQ-106",
    "用户画像标签体系 - RFM 分层与行为打标",
    "growth-mobile",
    "移动端用户增长系统",
    "P0",
    "Story",
    "doing",
    "张明远",
    "2025-05-28",
    "建立用户画像标签体系，为增长策略和推送分群提供基础数据。",
    ["支持 RFM 分层", "标签更新可配置", "支持人群导出"],
  ],
  [
    "REQ-107",
    "数据看板 - 实时客服并发与满意度趋势",
    "analytics-v3",
    "数据分析看板 V3",
    "P1",
    "Task",
    "doing",
    "王晨",
    "2025-05-10",
    "展示客服并发、工单趋势和满意度排行，支持运营复盘。",
    ["数据延迟低于 5 分钟", "支持维度筛选", "支持报表导出"],
  ],
  [
    "REQ-108",
    "数据库迁移方案设计",
    "cs-2",
    "客服平台 2.0 重构",
    "P1",
    "Task",
    "done",
    "赵磊",
    "2025-04-15",
    "整理客服平台 2.0 需要的数据结构变更和迁移步骤。",
    ["迁移步骤可回滚", "核心表结构评审通过", "迁移影响范围清晰"],
  ],
  [
    "REQ-109",
    "工单状态机定义 - 全生命周期设计",
    "cs-2",
    "客服平台 2.0 重构",
    "P1",
    "Task",
    "done",
    "张明远",
    "2025-04-10",
    "定义工单从创建、分配、处理、升级到关闭的完整状态机。",
    ["状态定义完整", "状态流转有权限控制", "异常回退路径明确"],
  ],
].map(([id, title, projectId, project, priority, type, status, assignee, dueDate, description, acceptanceCriteria]) => ({
  id,
  title,
  projectId,
  project,
  versionId: `${projectId}-default`,
  versionName: "默认版本",
  priority,
  type,
  status,
  assignee,
  dueDate,
  description,
  acceptanceCriteria,
})) as Requirement[];

export const knowledgeItems: KnowledgeItem[] = [
  ["项目知识库 / 客服平台", "用户分层与权限模型", "客服平台 2.0 多租户用户分层设计：角色定义、RBAC 权限矩阵", "张明远 / 5 分钟前", "收益：高"],
  ["项目知识库 / 客服平台", "第三方集成对接规范", "邮件通道、Facebook Messenger、WhatsApp API 对接方案", "李安 / 昨天 11:20", "2 次引用"],
  ["个人知识库", "需求优先级评估框架 RICE", "RICE 评分模型模板：触达、影响、信心、努力四维度", "张明远 / 昨天 16:00", "收益：高"],
  ["个人知识库", "竞品分析模板", "标准化竞品分析框架：功能矩阵、体验评分、差异化定位", "张明远 / 4 天前", "收益：中"],
  ["通用知识库", "PRD 标准模板 V4", "公司级 PRD 模板，包含概述、用户故事、功能清单、验收标准、数据埋点", "系统 / 更新于 04-10", "被引用 12 次"],
  ["通用知识库", "UI 组件库 - 基础规范", "颜色体系、字体层级、间距网格、组件使用原则", "系统 / 更新于 04-08", "被引用 8 次"],
].map(([scope, title, description, footer, metric]) => ({
  scope,
  title,
  description,
  footer,
  metric,
}));
