import { describe, expect, test } from "bun:test";
import type { LucideIcon } from "lucide-react";
import {
  BarChart3Icon,
  BookOpenIcon,
  DatabaseIcon,
  FilePlus2Icon,
  FileTextIcon,
  GlobeIcon,
  HistoryIcon,
  LayoutTemplateIcon,
  ListTodoIcon,
  MessageSquareIcon,
  PaperclipIcon,
  SearchIcon,
  UsersIcon,
  WrenchIcon,
} from "lucide-react";
import type { ToolCallTraceItem } from "@/components/chat/ToolCallTrace";
import {
  getToolMeta,
  isVisibleTraceTool,
  type ToolMeta,
} from "@/components/chat/toolMeta";

function tool(
  id: string,
  toolName: string,
  status: ToolCallTraceItem["status"] = "completed",
): ToolCallTraceItem {
  return {
    toolCallId: id,
    toolName,
    status,
    inputPreview: "{}",
  };
}

function expectMeta(toolName: string, label: string, icon: LucideIcon): void {
  const meta: ToolMeta = getToolMeta(toolName);
  expect(meta.label).toBe(label);
  expect(meta.icon).toBe(icon);
}

describe("isVisibleTraceTool", () => {
  test("hides orchestration and session-title metadata tools except failed spawn or ask_user calls", () => {
    expect(isVisibleTraceTool(tool("describe", "describe_task"))).toBe(false);
    expect(isVisibleTraceTool(tool("todos", "update_todos"))).toBe(false);
    expect(isVisibleTraceTool(tool("title", "set_session_title"))).toBe(false);
    expect(
      isVisibleTraceTool(tool("spawn-running", "spawn_subagents", "running")),
    ).toBe(false);
    expect(
      isVisibleTraceTool(
        tool("spawn-completed", "spawn_subagents", "completed"),
      ),
    ).toBe(false);
    expect(
      isVisibleTraceTool(tool("spawn-failed", "spawn_subagents", "failed")),
    ).toBe(true);
    expect(isVisibleTraceTool(tool("ask-ok", "ask_user", "completed"))).toBe(
      false,
    );
    expect(isVisibleTraceTool(tool("ask-failed", "ask_user", "failed"))).toBe(
      true,
    );
    expect(isVisibleTraceTool(tool("read", "read_blocks"))).toBe(true);
  });
});

describe("getToolMeta", () => {
  test("returns Chinese semantic labels for report and research tools", () => {
    expectMeta("read_blocks", "读取报告内容", FileTextIcon);
    expectMeta("get_document_digest", "读取文档大纲", FileTextIcon);
    expectMeta("preview_echarts", "预渲染图表", BarChart3Icon);
    expectMeta("configure_toc", "配置目录", LayoutTemplateIcon);
    expectMeta("add_comment", "添加批注", MessageSquareIcon);
    expectMeta("sandbox_mount_attachment", "挂载附件到沙箱", PaperclipIcon);
    expectMeta("deepseek_search", "检索权威资料", SearchIcon);
    expectMeta("deepseek_fetch", "读取网页原文", GlobeIcon);
    expectMeta(
      "panorama_report_universities",
      "查询高教全景数据",
      DatabaseIcon,
    );
    expectMeta("shuangyou_intel_list", "查询双优建设数据", DatabaseIcon);
    expectMeta("subjectcloud_overview", "查询学科云数据", DatabaseIcon);
  });

  test("covers report lifecycle and subagent orchestration tools", () => {
    expectMeta("get_protocol_guide", "查看协议指南", BookOpenIcon);
    expectMeta("create_report", "创建报告", FilePlus2Icon);
    expectMeta("rollback_revision", "回滚报告版本", HistoryIcon);
    expectMeta("list_subagents", "查看可用子代理", UsersIcon);
    expectMeta("spawn_subagents", "派发子代理", UsersIcon);
    expectMeta("describe_task", "播报工作状态", FileTextIcon);
    expectMeta("update_todos", "更新任务清单", ListTodoIcon);
  });

  test("covers every known report and runtime tool", () => {
    const toolNames = [
      "get_protocol_guide",
      "create_report",
      "list_reports",
      "get_document_digest",
      "read_as_markdown",
      "read_blocks",
      "apply_operations",
      "rollback_revision",
      "upsert_sources",
      "list_sources",
      "set_cover",
      "configure_toc",
      "configure_references",
      "configure_page_chrome",
      "update_style_and_css",
      "lint_document",
      "list_comments",
      "add_comment",
      "reply_comment",
      "preview_echarts",
      "commit_chart",
      "preview_report",
      "load_skill",
      "read_skill_resource",
      "sandbox_execute",
      "sandbox_read_file",
      "sandbox_write_file",
      "sandbox_mount_attachment",
      "ask_user",
      "set_session_title",
      "describe_task",
      "update_todos",
      "list_subagents",
      "spawn_subagents",
      "deepseek_search",
      "deepseek_fetch",
    ];
    for (const name of toolNames) {
      const meta = getToolMeta(name);
      expect(meta.label).not.toBe(name);
    }
  });

  test("falls back to the raw tool name and wrench icon", () => {
    expectMeta("unknown_runtime_tool", "unknown_runtime_tool", WrenchIcon);
  });
});
