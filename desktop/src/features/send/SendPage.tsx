import { useMemo, useState, type RefObject } from "react";
import {
  CheckCircle2,
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
import {
  errorMessage,
  type CodexInventory,
  type ConversationEntry,
  type CreatePackageReport,
} from "../../lib/types";

interface SendPageProps {
  headingRef: RefObject<HTMLHeadingElement | null>;
  inventory: CodexInventory | null;
}

export default function SendPage({ headingRef, inventory }: SendPageProps) {
  const [projects, setProjects] = useState<Set<string>>(new Set());
  const [conversations, setConversations] = useState<Set<string>>(new Set());
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [skills, setSkills] = useState(false);
  const [plugins, setPlugins] = useState(false);
  const [images, setImages] = useState(false);
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

  const hasContent = projects.size > 0 || conversations.size > 0 || skills || plugins || images;
  const canCreate = Boolean(inventory && hasContent && !busy);

  function toggle(setter: (value: Set<string>) => void, current: Set<string>, value: string) {
    const next = new Set(current);
    if (next.has(value)) next.delete(value);
    else next.add(value);
    setter(next);
  }

  async function handleCreate() {
    if (!inventory || !canCreate) return;
    setError(null);
    setBusy(true);
    try {
      const created = await createPackage({
        project_ids: [...projects],
        conversation_ids: [...conversations],
        include_skills: skills,
        include_plugins: plugins,
        include_generated_images: images,
      });
      if (!created) return;
      setReport(created);
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="page">
      <header className="page-header">
        <p className="eyebrow">SEND</p>
        <h1 ref={headingRef} tabIndex={-1}>发送交接</h1>
        <p className="page-description">项目文件和对话可以分开选择，对话按所属项目整理。</p>
      </header>

      <section className="workflow-section" aria-labelledby="send-projects-title">
        <div className="section-title-row">
          <div><span className="step-number">1</span><h2 id="send-projects-title">选择项目与对话</h2></div>
          <span className="selection-count">项目 {projects.size} · 对话 {conversations.size}</span>
        </div>
        <div className="project-list">
          {projectGroups.map((project) => (
            <ProjectChoice
              key={project.project_id}
              name={project.name}
              path={project.source_path}
              fileCount={project.file_count}
              conversations={project.conversations}
              projectSelected={projects.has(project.project_id)}
              expanded={expanded.has(project.project_id)}
              selectedConversations={conversations}
              onToggleProject={() => toggle(setProjects, projects, project.project_id)}
              onToggleExpanded={() => toggle(setExpanded, expanded, project.project_id)}
              onToggleConversation={(id) => toggle(setConversations, conversations, id)}
            />
          ))}
          {!projectGroups.length && <p className="empty-state">未检测到真实的项目文件夹</p>}
          {unassociatedConversations.length > 0 && (
            <ProjectChoice
              name="未归属项目的对话"
              path="不包含项目文件"
              fileCount={null}
              conversations={unassociatedConversations}
              projectSelected={false}
              expanded={expanded.has("unassociated")}
              selectedConversations={conversations}
              onToggleExpanded={() => toggle(setExpanded, expanded, "unassociated")}
              onToggleConversation={(id) => toggle(setConversations, conversations, id)}
            />
          )}
        </div>
      </section>

      <section className="workflow-section" aria-labelledby="send-content-title">
        <div className="section-title-row">
          <div><span className="step-number">2</span><h2 id="send-content-title">其他 Codex 内容</h2></div>
          <span className="selection-count">按需选择</span>
        </div>
        <div className="choice-list compact-choices">
          <label className="choice-row"><input type="checkbox" checked={skills} onChange={(event) => setSkills(event.target.checked)} /><Sparkles aria-hidden="true" /><span><strong>Skills</strong><small>{inventory?.counts.skills ?? 0} 项</small></span></label>
          <label className="choice-row"><input type="checkbox" checked={plugins} onChange={(event) => setPlugins(event.target.checked)} /><Puzzle aria-hidden="true" /><span><strong>Plugins</strong><small>{inventory?.counts.plugins ?? 0} 项</small></span></label>
          <label className="choice-row"><input type="checkbox" checked={images} onChange={(event) => setImages(event.target.checked)} /><Image aria-hidden="true" /><span><strong>生成图片</strong><small>{inventory?.counts.generated_images ?? 0} 项</small></span></label>
        </div>
      </section>

      <section className="workflow-section" aria-labelledby="send-output-title">
        <div className="section-title-row"><div><span className="step-number">3</span><h2 id="send-output-title">输出位置</h2></div></div>
        <div className="form-row"><div className="form-label"><FileArchive aria-hidden="true" /><span><strong>ReHome 包</strong><small>创建时通过系统窗口选择 .rehome 保存位置</small></span></div></div>
        <div className="command-row"><p>{hasContent ? "选择已完成，可以创建迁移包" : "请选择项目文件、对话或其他 Codex 内容"}</p><button className="command-button" type="button" disabled={!canCreate} onClick={() => void handleCreate()}>{busy ? <LoaderCircle className="spin" aria-hidden="true" /> : <PackagePlus aria-hidden="true" />}创建 ReHome 包</button></div>
        {error && <p className="inline-state status-error" role="alert">{error}</p>}
      </section>

      {report && (
        <section className="result-panel" aria-labelledby="package-result-title">
          <div className="section-title-row"><div><CheckCircle2 aria-hidden="true" /><h2 id="package-result-title">迁移包已创建</h2></div><span className="status status-success">校验通过</span></div>
          <div className="result-grid"><span>大小<strong>{formatBytes(report.bytes_written)}</strong></span><span>SHA-256<strong className="hash-text">{report.archive_hash}</strong></span><span>内容<strong>{report.counts.project_files} 个项目文件 / {report.counts.conversations} 个对话</strong></span></div>
          <div className="result-actions"><code>{report.package_path}</code><button className="secondary-button" type="button" onClick={() => void openPath(report.reveal_id)}><FolderOpen aria-hidden="true" />在文件夹中显示</button></div>
        </section>
      )}
    </div>
  );
}

interface ProjectChoiceProps {
  name: string;
  path: string;
  fileCount: number | null;
  conversations: ConversationEntry[];
  projectSelected: boolean;
  expanded: boolean;
  selectedConversations: Set<string>;
  onToggleProject?: () => void;
  onToggleExpanded: () => void;
  onToggleConversation: (id: string) => void;
}

function ProjectChoice({
  name,
  path,
  fileCount,
  conversations,
  projectSelected,
  expanded,
  selectedConversations,
  onToggleProject,
  onToggleExpanded,
  onToggleConversation,
}: ProjectChoiceProps) {
  return (
    <div className="project-choice">
      <div className="project-choice-header">
        {onToggleProject ? (
          <label className="project-file-toggle">
            <input type="checkbox" checked={projectSelected} onChange={onToggleProject} aria-label={`选择项目 ${name}`} />
            <span className="project-copy"><strong>{name}</strong><code>{path}</code></span>
          </label>
        ) : (
          <span className="project-copy project-copy-unassociated"><strong>{name}</strong><small>{path}</small></span>
        )}
        <button
          className="project-expand"
          type="button"
          aria-expanded={expanded}
          aria-label={`${expanded ? "收起" : "展开"}项目 ${name}`}
          onClick={onToggleExpanded}
        >
          <span>{conversations.length} 个对话{fileCount !== null && ` · ${fileCount || "已检测"} 个文件`}</span>
          {expanded ? <ChevronDown aria-hidden="true" /> : <ChevronRight aria-hidden="true" />}
        </button>
      </div>
      {expanded && (
        <div className="project-conversations" aria-label={`${name} 的对话`}>
          {conversations.map((conversation) => (
            <label className="conversation-choice" key={conversation.task_id}>
              <input type="checkbox" checked={selectedConversations.has(conversation.task_id)} onChange={() => onToggleConversation(conversation.task_id)} aria-label={`选择对话 ${conversation.title}`} />
              <MessageSquareText aria-hidden="true" />
              <span><strong>{conversation.title}</strong><small>{formatDate(conversation.updated_at)}</small></span>
            </label>
          ))}
          {!conversations.length && <p className="project-empty">这个项目下暂无可迁移对话</p>}
        </div>
      )}
    </div>
  );
}

function formatDate(value: string): string {
  if (!value) return "时间未知";
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString("zh-CN", { hour12: false });
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
