import { useMemo, useState, type ReactNode, type RefObject } from "react";
import {
  CheckCircle2,
  Bot,
  ChevronDown,
  ChevronRight,
  FileArchive,
  FolderOpen,
  Image,
  LoaderCircle,
  MessageSquareText,
  PackagePlus,
  Puzzle,
  Sparkles,
} from "lucide-react";

import { createPackage, openPath } from "../../lib/api";
import { useI18n } from "../../lib/i18n";
import {
  errorMessage,
  type CodexInventory,
  type ConversationEntry,
  type CreatePackageReport,
  type OptionalContentEntry,
} from "../../lib/types";

interface SendPageProps {
  headingRef: RefObject<HTMLHeadingElement | null>;
  inventory: CodexInventory | null;
  onOperationStart: () => void;
  onOperationEnd: () => void;
}

export default function SendPage({
  headingRef,
  inventory,
  onOperationStart,
  onOperationEnd,
}: SendPageProps) {
  const { t } = useI18n();
  const [projects, setProjects] = useState<Set<string>>(new Set());
  const [conversations, setConversations] = useState<Set<string>>(new Set());
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [skills, setSkills] = useState<Set<string>>(new Set());
  const [plugins, setPlugins] = useState<Set<string>>(new Set());
  const [images, setImages] = useState<Set<string>>(new Set());
  const [report, setReport] = useState<CreatePackageReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const projectGroups = useMemo(() => {
    if (!inventory) return [];
    return inventory.projects.map((project) => ({
      ...project,
      conversations: inventory.conversations.filter(
        (conversation) => conversation.project_id === project.project_id,
      ),
    }));
  }, [inventory]);
  const unassociatedConversations = useMemo(
    () => inventory?.conversations.filter((conversation) => conversation.project_id === null) ?? [],
    [inventory],
  );

  const hasContent =
    projects.size + conversations.size + skills.size + plugins.size + images.size > 0;
  const hasSelectableContent = Boolean(
    inventory &&
      inventory.projects.filter((project) => project.source_available).length +
        inventory.conversations.length +
        inventory.skills.length +
        inventory.plugins.length +
        inventory.generated_images.length >
        0,
  );
  const allContentSelected = Boolean(
    inventory &&
      hasSelectableContent &&
      inventory.projects
        .filter((project) => project.source_available)
        .every((project) => projects.has(project.project_id)) &&
      inventory.conversations.every((conversation) => conversations.has(conversation.task_id)) &&
      inventory.skills.every((skill) => skills.has(skill.content_id)) &&
      inventory.plugins.every((plugin) => plugins.has(plugin.content_id)) &&
      inventory.generated_images.every((image) => images.has(image.content_id)),
  );
  const canCreate = Boolean(inventory && hasContent && !busy);

  function toggle(setter: (value: Set<string>) => void, current: Set<string>, value: string) {
    const next = new Set(current);
    if (next.has(value)) next.delete(value);
    else next.add(value);
    setter(next);
  }

  function selectRecommended(items: ConversationEntry[]) {
    const next = new Set(conversations);
    for (const item of items) next.delete(item.task_id);
    for (const item of items) {
      if (!item.classification) next.add(item.task_id);
    }
    setConversations(next);
  }

  function toggleProject(projectId: string, items: ConversationEntry[]) {
    const nextProjects = new Set(projects);
    const nextConversations = new Set(conversations);
    if (nextProjects.has(projectId)) {
      nextProjects.delete(projectId);
      for (const item of items) nextConversations.delete(item.task_id);
    } else {
      nextProjects.add(projectId);
      for (const item of items) nextConversations.add(item.task_id);
      setExpanded((current) => new Set(current).add(projectId));
    }
    setProjects(nextProjects);
    setConversations(nextConversations);
  }

  function toggleAllContent() {
    if (!inventory) return;

    if (allContentSelected) {
      setProjects(new Set());
      setConversations(new Set());
      setSkills(new Set());
      setPlugins(new Set());
      setImages(new Set());
      return;
    }

    setProjects(
      new Set(
        inventory.projects
          .filter((project) => project.source_available)
          .map((project) => project.project_id),
      ),
    );
    setConversations(new Set(inventory.conversations.map((conversation) => conversation.task_id)));
    setSkills(new Set(inventory.skills.map((skill) => skill.content_id)));
    setPlugins(new Set(inventory.plugins.map((plugin) => plugin.content_id)));
    setImages(new Set(inventory.generated_images.map((image) => image.content_id)));
  }

  async function handleCreate() {
    if (!inventory || !canCreate) return;
    setError(null);
    setBusy(true);
    onOperationStart();
    try {
      const created = await createPackage({
        project_ids: [...projects],
        conversation_ids: [...conversations],
        skill_ids: [...skills],
        plugin_ids: [...plugins],
        generated_image_ids: [...images],
      });
      if (created) {
        setReport(created);
        try {
          await openPath(created.reveal_id);
        } catch {
          setError(t("迁移包已创建在：{path}\n但没能自动打开所在文件夹。", { path: created.package_path }));
        }
      }
    } catch (caught) {
      const message = errorMessage(caught);
      setError(
        message.includes("source file kept changing while being copied")
          ? t("有文件在打包过程中仍被修改。请稍等几秒后重试；如果反复出现，请先完全退出 Codex。\n{message}", { message })
          : message,
      );
    } finally {
      setBusy(false);
      onOperationEnd();
    }
  }

  return (
    <div className="page">
      <header className="page-header page-header-with-action">
        <div>
          <p className="eyebrow">EXPORT</p>
          <h1 ref={headingRef} tabIndex={-1}>{t("导出 Codex 数据")}</h1>
          <p className="page-description">{t("在原电脑选择要带走的项目、对话和其他 Codex 内容。")}</p>
        </div>
        <label className="global-select-toggle">
          <input
            type="checkbox"
            checked={allContentSelected}
            onChange={toggleAllContent}
            disabled={!hasSelectableContent}
            aria-label={t("全选迁移内容")}
          />
          <span>
            <strong>{t("全选迁移内容")}</strong>
            <small>{t("项目、对话和 Codex 内容")}</small>
          </span>
        </label>
      </header>

      <section className="workflow-section" aria-labelledby="send-projects-title">
        <div className="section-title-row">
          <div><span className="step-number">1</span><h2 id="send-projects-title">{t("选择项目与对话")}</h2></div>
          <span className="selection-count">{t("项目 {projects} · 对话 {conversations}", { projects: projects.size, conversations: conversations.size })}</span>
        </div>
        <div className="project-list">
          {projectGroups.map((project) => (
            <ProjectChoice
              key={project.project_id}
              name={project.name}
              path={formatDisplayPath(project.source_path)}
              fileCount={project.file_count}
              sourceAvailable={project.source_available}
              conversations={project.conversations}
              projectSelected={projects.has(project.project_id)}
              expanded={expanded.has(project.project_id)}
              selectedConversations={conversations}
              onToggleProject={() => toggleProject(project.project_id, project.conversations)}
              onToggleExpanded={() => toggle(setExpanded, expanded, project.project_id)}
              onToggleConversation={(id) => toggle(setConversations, conversations, id)}
              onSelectRecommended={() => selectRecommended(project.conversations)}
            />
          ))}
          {!projectGroups.length && <p className="empty-state">{t("未检测到 Codex 已登记的本机项目")}</p>}
          {unassociatedConversations.length > 0 && (
            <ProjectChoice
              name={t("未归属项目的对话")}
              path={t("只迁移对话，不包含项目文件")}
              fileCount={null}
              conversations={unassociatedConversations}
              projectSelected={false}
              expanded={expanded.has("unassociated")}
              selectedConversations={conversations}
              onToggleExpanded={() => toggle(setExpanded, expanded, "unassociated")}
              onToggleConversation={(id) => toggle(setConversations, conversations, id)}
              onSelectRecommended={() => selectRecommended(unassociatedConversations)}
            />
          )}
        </div>
      </section>

      <section className="workflow-section" aria-labelledby="send-content-title">
        <div className="section-title-row">
          <div><span className="step-number">2</span><h2 id="send-content-title">{t("其他 Codex 内容")}</h2></div>
          <span className="selection-count">{t("都不是必选项")}</span>
        </div>
        <div className="optional-content-list">
          <OptionalContentGroup
            id="skills"
            title="Skills"
            description={t("迁移你希望在新电脑继续使用的能力")}
            icon={<Sparkles aria-hidden="true" />}
            items={inventory?.skills ?? []}
            selected={skills}
            expanded={expanded.has("skills")}
            onToggleExpanded={() => toggle(setExpanded, expanded, "skills")}
            onChange={setSkills}
          />
          <OptionalContentGroup
            id="plugins"
            title="Plugins"
            description={t("通常可以在新电脑重装，也可以选择带走")}
            icon={<Puzzle aria-hidden="true" />}
            items={inventory?.plugins ?? []}
            selected={plugins}
            expanded={expanded.has("plugins")}
            onToggleExpanded={() => toggle(setExpanded, expanded, "plugins")}
            onChange={setPlugins}
          />
          <OptionalContentGroup
            id="images"
            title={t("生成图片")}
            description={t("只在需要保留历史生成物时选择")}
            icon={<Image aria-hidden="true" />}
            items={inventory?.generated_images ?? []}
            selected={images}
            expanded={expanded.has("images")}
            onToggleExpanded={() => toggle(setExpanded, expanded, "images")}
            onChange={setImages}
          />
        </div>
      </section>

      <section className="workflow-section" aria-labelledby="send-output-title">
        <div className="section-title-row"><div><span className="step-number">3</span><h2 id="send-output-title">{t("保存迁移包")}</h2></div></div>
        <div className="form-row"><div className="form-label"><FileArchive aria-hidden="true" /><span><strong>{t("ReHome 迁移包")}</strong><small>{t("通过系统窗口选择 .rehome 文件的保存位置")}</small></span></div></div>
        <div className="command-row">
          <p role={busy ? "status" : undefined}>
            {busy
              ? t("正在创建迁移包。内容较多时可能需要几分钟，请保持 ReHome 打开。")
              : hasContent
                ? t("选择已完成，可以创建迁移包")
                : t("请选择需要迁移的内容")}
          </p>
          <button className="command-button" type="button" disabled={!canCreate} onClick={() => void handleCreate()}>
            {busy ? <LoaderCircle className="spin" aria-hidden="true" /> : <PackagePlus aria-hidden="true" />}
            {t(busy ? "正在创建迁移包" : "创建迁移包")}
          </button>
        </div>
        {error && <p className="inline-state status-error" role="alert">{error}</p>}
      </section>

      {report && (
        <section className="result-panel" aria-labelledby="package-result-title">
          <div className="section-title-row"><div><CheckCircle2 aria-hidden="true" /><h2 id="package-result-title">{t("迁移包已创建")}</h2></div><span className="status status-success">{t("校验通过")}</span></div>
          <div className="result-grid"><span>{t("大小")}<strong>{formatBytes(report.bytes_written)}</strong></span><span>SHA-256<strong className="hash-text">{report.archive_hash}</strong></span><span>{t("内容")}<strong>{t("{files} 个项目文件 / {conversations} 个对话", { files: report.counts.project_files, conversations: report.counts.conversations })}</strong></span></div>
          <div className="result-actions"><code>{report.package_path}</code><button className="secondary-button" type="button" onClick={() => void openPath(report.reveal_id)}><FolderOpen aria-hidden="true" />{t("在文件夹中显示")}</button></div>
        </section>
      )}
    </div>
  );
}

interface ProjectChoiceProps {
  name: string;
  path: string;
  fileCount: number | null;
  sourceAvailable?: boolean;
  conversations: ConversationEntry[];
  projectSelected: boolean;
  expanded: boolean;
  selectedConversations: Set<string>;
  onToggleProject?: () => void;
  onToggleExpanded: () => void;
  onToggleConversation: (id: string) => void;
  onSelectRecommended: () => void;
}

function ProjectChoice({ name, path, fileCount, sourceAvailable = true, conversations, projectSelected, expanded, selectedConversations, onToggleProject, onToggleExpanded, onToggleConversation, onSelectRecommended }: ProjectChoiceProps) {
  const { locale, t } = useI18n();
  const subagents = conversations.filter((conversation) => conversation.classification).length;
  const mainConversations = conversations.length - subagents;
  return (
    <div className={`project-choice${sourceAvailable ? "" : " project-choice-missing"}`}>
      <div className="project-choice-header">
        {onToggleProject ? (
          <label className="project-file-toggle">
            <input type="checkbox" checked={projectSelected} onChange={onToggleProject} disabled={!sourceAvailable} aria-label={t("选择项目 {name}", { name })} />
            <span className="project-copy">
              <strong>{name}</strong>
              <code>{path}</code>
              {!sourceAvailable && <small>{t("项目文件夹已不存在，仅可迁移下面的对话")}</small>}
            </span>
          </label>
        ) : (
          <span className="project-copy project-copy-unassociated"><strong>{name}</strong><small>{path}</small></span>
        )}
        <button className="project-expand" type="button" aria-expanded={expanded} aria-label={t(expanded ? "收起项目 {name}" : "展开项目 {name}", { name })} onClick={onToggleExpanded}>
          <span>{t("{count} 个对话", { count: conversations.length })}{fileCount !== null && (sourceAvailable ? ` · ${fileCount ? t("{count} 个文件", { count: fileCount }) : t("项目文件将在打包时统计")}` : ` · ${t("项目文件缺失")}`)}</span>
          {expanded ? <ChevronDown aria-hidden="true" /> : <ChevronRight aria-hidden="true" />}
        </button>
      </div>
      {expanded && (
        <div className="project-conversations" aria-label={t("{name} 的对话", { name })}>
          {conversations.length > 0 && (
            <div className="conversation-toolbar">
              <span>{t("主对话 {main} · 子 Agent {subagents} · 单独勾对话不含项目文件", { main: mainConversations, subagents })}</span>
              {mainConversations > 0 && <button type="button" onClick={onSelectRecommended}>{t("只选主对话")}</button>}
            </div>
          )}
          {conversations.map((conversation) => (
            <label className="conversation-choice" key={conversation.task_id}>
              <input type="checkbox" checked={selectedConversations.has(conversation.task_id)} onChange={() => onToggleConversation(conversation.task_id)} aria-label={t("选择对话 {name}", { name: conversation.title })} />
              {conversation.classification ? <Bot aria-hidden="true" /> : <MessageSquareText aria-hidden="true" />}
              <span>
                <strong>{conversation.title}</strong>
                <small className="conversation-details">
                  <span className={conversation.classification ? "conversation-badge badge-subagent" : "conversation-badge badge-main"}>{conversation.classification ? `${t("子 Agent")}${conversation.classification.depth ? ` · L${conversation.classification.depth}` : ""}` : t("主对话")}</span>
                  <span>{t(conversation.classification ? "辅助记录，通常可不迁移" : "建议迁移")}</span>
                  <time>{formatDate(conversation.updated_at, locale)}</time>
                </small>
              </span>
            </label>
          ))}
          {!conversations.length && <p className="project-empty">{t("这个项目下暂无可迁移对话")}</p>}
        </div>
      )}
    </div>
  );
}

interface OptionalContentGroupProps {
  id: string;
  title: string;
  description: string;
  icon: ReactNode;
  items: OptionalContentEntry[];
  selected: Set<string>;
  expanded: boolean;
  onToggleExpanded: () => void;
  onChange: (value: Set<string>) => void;
}

function OptionalContentGroup({ id, title, description, icon, items, selected, expanded, onToggleExpanded, onChange }: OptionalContentGroupProps) {
  const { t } = useI18n();
  const allSelected = items.length > 0 && items.every((item) => selected.has(item.content_id));
  function toggleItem(contentId: string) {
    const next = new Set(selected);
    if (next.has(contentId)) next.delete(contentId);
    else next.add(contentId);
    onChange(next);
  }
  function toggleAll() {
    onChange(allSelected ? new Set() : new Set(items.map((item) => item.content_id)));
  }

  return (
    <div className="optional-content-group">
      <div className="optional-content-header">
        <label className="optional-all-toggle">
          <input type="checkbox" checked={allSelected} onChange={toggleAll} disabled={!items.length} aria-label={t("全选 {name}", { name: title })} />
          {icon}
          <span><strong>{title}</strong><small>{description}</small></span>
        </label>
        <button type="button" className="project-expand" aria-expanded={expanded} aria-controls={`optional-${id}`} onClick={onToggleExpanded}>
          <span>{t("已选 {selected} / {total}", { selected: selected.size, total: items.length })}</span>
          {expanded ? <ChevronDown aria-hidden="true" /> : <ChevronRight aria-hidden="true" />}
        </button>
      </div>
      {expanded && (
        <div className="optional-items" id={`optional-${id}`}>
          {items.map((item) => (
            <div className={`optional-item${item.thumbnail_data_url ? " optional-item-image" : ""}`} key={item.content_id}>
              <input type="checkbox" checked={selected.has(item.content_id)} onChange={() => toggleItem(item.content_id)} aria-label={t("选择 {name}", { name: item.name })} />
              {item.thumbnail_data_url && <img className="image-thumbnail" src={item.thumbnail_data_url} alt="" />}
              <span><strong>{item.name}</strong><small>{item.relative_path}</small></span>
              <small className="item-size">{formatBytes(item.size_bytes)}</small>
              {item.reveal_id && (
                <button className="icon-button image-reveal-button" type="button" title={t("在文件夹中显示")} aria-label={t("在文件夹中显示 {name}", { name: item.name })} onClick={() => void openPath(item.reveal_id!)}>
                  <FolderOpen aria-hidden="true" />
                </button>
              )}
            </div>
          ))}
          {!items.length && <p className="project-empty">{t("没有检测到这类内容")}</p>}
        </div>
      )}
    </div>
  );
}

function formatDisplayPath(value: string): string {
  return value.startsWith("\\\\?\\") ? value.slice(4) : value;
}

function formatDate(value: string, locale: "zh-CN" | "en"): string {
  if (!value) return locale === "en" ? "Time unknown" : "时间未知";
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString(locale, { hour12: false });
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
