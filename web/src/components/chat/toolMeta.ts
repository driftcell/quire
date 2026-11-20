import type { LucideIcon } from "lucide-react";
import {
  BarChart3Icon,
  BookOpenIcon,
  EyeIcon,
  FilePlus2Icon,
  FileTextIcon,
  FilesIcon,
  HelpCircleIcon,
  HistoryIcon,
  LayoutTemplateIcon,
  Link2Icon,
  MessageSquareIcon,
  PenLineIcon,
  ShieldCheckIcon,
  TerminalIcon,
  TypeIcon,
  UsersIcon,
  WrenchIcon,
} from "lucide-react";
import type { ToolCallTraceItem } from "@/components/chat/ToolCallTrace";

export interface ToolMeta {
  label: string;
  icon: LucideIcon;
}

const TOOL_META: Record<string, ToolMeta> = {
  get_protocol_guide: { label: "查看协议指南", icon: BookOpenIcon },
  create_report: { label: "创建报告", icon: FilePlus2Icon },
  list_reports: { label: "查看报告列表", icon: FilesIcon },
  read_blocks: { label: "读取报告内容", icon: FileTextIcon },
  get_document_digest: { label: "读取文档大纲", icon: FileTextIcon },
  read_as_markdown: { label: "读取报告正文", icon: FileTextIcon },
  sandbox_read_file: { label: "读取文件", icon: FileTextIcon },
  apply_operations: { label: "写入报告内容", icon: PenLineIcon },
  sandbox_write_file: { label: "写入文件", icon: PenLineIcon },
  rollback_revision: { label: "回滚报告版本", icon: HistoryIcon },
  preview_echarts: { label: "预渲染图表", icon: BarChart3Icon },
  commit_chart: { label: "写入图表", icon: BarChart3Icon },
  lint_document: { label: "校验报告格式", icon: ShieldCheckIcon },
  set_cover: { label: "配置封面", icon: LayoutTemplateIcon },
  configure_toc: { label: "配置目录", icon: LayoutTemplateIcon },
  configure_references: { label: "配置参考文献", icon: LayoutTemplateIcon },
  configure_page_chrome: { label: "配置页眉页脚", icon: LayoutTemplateIcon },
  update_style_and_css: { label: "更新样式与 CSS", icon: LayoutTemplateIcon },
  upsert_sources: { label: "登记数据来源", icon: Link2Icon },
  list_comments: { label: "查看批注", icon: MessageSquareIcon },
  add_comment: { label: "添加批注", icon: MessageSquareIcon },
  reply_comment: { label: "回复批注", icon: MessageSquareIcon },
  preview_report: { label: "预览排版效果", icon: EyeIcon },
  load_skill: { label: "加载专业规范", icon: BookOpenIcon },
  read_skill_resource: { label: "读取规范资源", icon: BookOpenIcon },
  sandbox_execute: { label: "执行分析脚本", icon: TerminalIcon },
  ask_user: { label: "向用户确认", icon: HelpCircleIcon },
  set_session_title: { label: "更新会话标题", icon: TypeIcon },
  list_subagents: { label: "查看可用子代理", icon: UsersIcon },
  spawn_subagents: { label: "派发子代理", icon: UsersIcon },
  describe_task: { label: "整理任务说明", icon: FileTextIcon },
};

/**
 * Internal orchestration tools never render as their own trace row. The
 * subagent cards already represent their effect, so showing the tool too would
 * duplicate the loading state. A failed `spawn_subagents` call is the
 * exception: keep it visible for debugging.
 */
export function isVisibleTraceTool(tool: ToolCallTraceItem): boolean {
  if (tool.toolName === "describe_task") {
    return false;
  }
  if (tool.toolName === "spawn_subagents") {
    return tool.status === "failed";
  }
  return true;
}

export function getToolMeta(toolName: string): ToolMeta {
  return TOOL_META[toolName] ?? { label: toolName, icon: WrenchIcon };
}
