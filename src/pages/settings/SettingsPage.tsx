import { Select, Switch } from "antd";
import { SettingRow, SettingsSection } from "../../components/settings/SettingsRow";

export function SettingsPage() {
  return (
    <div className="h-full overflow-y-auto p-4">
      <div className="max-w-[600px] space-y-4">
        <SettingsSection title="个人偏好">
          <SettingRow label="默认项目进入视图" desc="打开项目时默认展示的视图" control={<Select defaultValue="需求列表" className="w-36" options={["需求列表", "Epic 视图", "时间线"].map((value) => ({ value }))} />} />
          <SettingRow label="新需求默认优先级" control={<Select defaultValue="P1 - 重要" className="w-36" options={["P1 - 重要", "P0 - 紧急", "P2 - 一般"].map((value) => ({ value }))} />} />
          <SettingRow label="邮件通知" desc="需求分配、状态变更时接收邮件" control={<Switch defaultChecked />} />
        </SettingsSection>
        <SettingsSection title="AI 生成配置">
          <SettingRow label="默认文档模板" control={<Select defaultValue="标准 PRD 模板" className="w-40" options={["标准 PRD 模板", "简洁模板", "详细技术文档模板"].map((value) => ({ value }))} />} />
          <SettingRow label="生成语言" control={<Select defaultValue="中文" className="w-32" options={["中文", "English", "中英双语"].map((value) => ({ value }))} />} />
          <SettingRow label="自动引用知识库" desc="生成文档时自动检索并引用相关知识点" control={<Switch defaultChecked />} />
        </SettingsSection>
        <SettingsSection title="知识库管理">
          <SettingRow label="自动同步团队文档" desc="订阅指定 Confluence/Notion 空间" control={<Switch />} />
          <SettingRow label="通用知识库共享范围" control={<Select defaultValue="仅当前团队" className="w-36" options={["仅当前团队", "全组织", "仅自己"].map((value) => ({ value }))} />} />
        </SettingsSection>
      </div>
    </div>
  );
}
