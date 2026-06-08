import { Button, Card, Form, Input, Radio, Select } from "antd";
import { useWorkspaceStore } from "../../store";

export function GeneratePage() {
  const generated = useWorkspaceStore((state) => state.generated);
  const showGenerated = useWorkspaceStore((state) => state.showGenerated);
  const projects = useWorkspaceStore((state) => state.projects);
  const requirements = useWorkspaceStore((state) => state.requirements);

  return (
    <div className="h-full overflow-y-auto p-4">
      <Card className="desktop-card max-w-[720px]" title="AI 生成文档">
        <p className="mb-4 text-xs text-[#667085]">
          选择项目和需求，基于知识库和需求描述生成原型文档或需求文档。
        </p>
        <Form layout="vertical">
          <Form.Item label="选择项目">
            <Select value={projects[0]?.id} options={projects.map((item) => ({ value: item.id, label: item.name }))} />
          </Form.Item>
          <Form.Item label="选择需求">
            <Select
              mode="multiple"
              defaultValue={requirements.slice(0, 2).map((item) => item.id)}
              options={requirements.slice(0, 4).map((item) => ({ value: item.id, label: `${item.id} ${item.title}` }))}
            />
          </Form.Item>
          <Form.Item label="文档类型">
            <Radio.Group defaultValue="prd">
              <Radio value="prd">需求文档 PRD</Radio>
              <Radio value="prototype">原型说明文档</Radio>
              <Radio value="tech">技术方案概要</Radio>
            </Radio.Group>
          </Form.Item>
          <Form.Item label="补充说明">
            <Input.TextArea rows={4} placeholder="例如：重点描述用户交互流程、异常状态、验收标准..." />
          </Form.Item>
          <Button type="primary" onClick={showGenerated}>
            生成文档
          </Button>
        </Form>
        {generated && (
          <div className="mt-4 rounded-md border border-[#d8dde5] bg-white p-4 text-[13px] leading-7">
            <h2 className="m-0 mb-2 text-lg font-bold">需求文档：统一工单中心</h2>
            <p>
              <b>版本：</b>v1.0 / <b>日期：</b>2025-04-18 / <b>作者：</b>AI 生成
            </p>
            <h3 className="mb-1 mt-3 text-[15px] font-semibold">1. 概述</h3>
            <p>统一工单中心是客服平台 2.0 的核心模块，目标是统一展示邮件、在线聊天、电话回拨三大渠道的客户请求。</p>
            <h3 className="mb-1 mt-3 text-[15px] font-semibold">2. 功能清单</h3>
            <table className="w-full border-collapse bg-white text-xs">
              <tbody>
                <tr>
                  <th className="border border-[#d8dde5] p-2 text-left">功能模块</th>
                  <th className="border border-[#d8dde5] p-2 text-left">优先级</th>
                  <th className="border border-[#d8dde5] p-2 text-left">描述</th>
                </tr>
                <tr>
                  <td className="border border-[#d8dde5] p-2">工单聚合列表</td>
                  <td className="border border-[#d8dde5] p-2">P0</td>
                  <td className="border border-[#d8dde5] p-2">统一展示所有渠道工单，支持筛选、排序、搜索</td>
                </tr>
                <tr>
                  <td className="border border-[#d8dde5] p-2">工单详情面板</td>
                  <td className="border border-[#d8dde5] p-2">P0</td>
                  <td className="border border-[#d8dde5] p-2">展示客户信息、渠道来源、历史交互、处理记录</td>
                </tr>
              </tbody>
            </table>
          </div>
        )}
      </Card>
    </div>
  );
}
